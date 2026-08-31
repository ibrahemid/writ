import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";
import { themeStore } from "../../stores/global/theme";
import { TOKEN_GROUPS, GROUP_LABELS } from "../../types/theme";
import type { WritConfig } from "../../types/config";

const h = vi.hoisted(() => ({
  save: vi.fn().mockResolvedValue(undefined),
  config: vi.fn(),
  focusEditor: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => h.config(),
    save: h.save,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import ThemeEditor, { openThemeEditor, closeThemeEditor } from "../../components/ThemeEditor/ThemeEditor";

function baseConfig(): Partial<WritConfig> {
  return { theme: { preset: "writ-light", overrides: {} } };
}

function groupTitles(container: HTMLElement): string[] {
  return Array.from(container.querySelectorAll(".theme-editor-group-title")).map(
    (el) => el.textContent ?? "",
  );
}

function pickerFor(container: HTMLElement, key: string): HTMLInputElement {
  const input = container.querySelector<HTMLInputElement>(`input[data-token="${key}"]`);
  if (!input) throw new Error(`no picker for ${key}`);
  return input;
}

describe("ThemeEditor", () => {
  beforeEach(() => {
    h.save.mockReset().mockResolvedValue(undefined);
    h.config.mockReset().mockReturnValue(baseConfig());
    themeStore.setAppearance({ polarity: "light", accent: "pine", prose_face: "system" });
    themeStore.loadConfig({ preset: "writ-light", overrides: {} });
  });

  afterEach(() => {
    closeThemeEditor();
    cleanup();
  });

  it("renders every token group", () => {
    const { container } = render(() => <ThemeEditor />);
    openThemeEditor();
    expect(groupTitles(container)).toHaveLength(TOKEN_GROUPS.length);
  });

  it("group titles are sentence case", () => {
    const { container } = render(() => <ThemeEditor />);
    openThemeEditor();
    expect(groupTitles(container)).toEqual(TOKEN_GROUPS.map((g) => GROUP_LABELS[g]));
    expect(groupTitles(container)).toEqual(["Background", "Text", "Borders", "Accent", "Status", "Syntax"]);
  });

  it("addresses a group's default leaf by the bare token name", () => {
    const { container } = render(() => <ThemeEditor />);
    openThemeEditor();
    expect(container.querySelector('input[data-token="fg"]')).not.toBeNull();
    expect(container.querySelector('input[data-token="fg.default"]')).toBeNull();
    expect(pickerFor(container, "fg").value).toBe("#1c1a17");
  });

  it("an override survives a save/load round trip", async () => {
    const { container } = render(() => <ThemeEditor />);
    openThemeEditor();
    fireEvent.input(pickerFor(container, "bg.canvas"), { target: { value: "#123456" } });
    fireEvent.click(container.querySelector<HTMLButtonElement>("[data-action='save-theme']")!);
    await waitFor(() => expect(h.save).toHaveBeenCalledTimes(1));

    const saved = h.save.mock.calls[0][0] as WritConfig;
    expect(saved.theme.overrides).toEqual({ "bg.canvas": "#123456" });

    themeStore.loadConfig({ preset: "writ-light", overrides: {} });
    expect(themeStore.resolvedTokens()["bg.canvas"]).toBe("#ffffff");
    themeStore.loadConfig(saved.theme);
    expect(themeStore.resolvedTokens()["bg.canvas"]).toBe("#123456");
    expect(pickerFor(container, "bg.canvas").value).toBe("#123456");
  });

  it("legacy override keys migrate on load", () => {
    const { container } = render(() => <ThemeEditor />);
    // A config written before the group rename. border.focus named the focus
    // ring, which the accent absorbed, so it has nowhere to land.
    themeStore.loadConfig({
      preset: "writ-light",
      overrides: { "surface.background": "#123456", "foreground.subtle": "#abcdef", "border.focus": "#ff0000" },
    });
    openThemeEditor();
    expect(pickerFor(container, "bg.canvas").value).toBe("#123456");
    expect(pickerFor(container, "fg.faint").value).toBe("#abcdef");
    expect(themeStore.overrides()).toEqual({ "bg.canvas": "#123456", "fg.faint": "#abcdef" });
  });

  it("the preset select names the half of the pair that renders", () => {
    // The swatches below it are the active preset's, so naming the stored half
    // would label a dark palette "Writ Light".
    const { container } = render(() => <ThemeEditor />);
    themeStore.setAppearance({ polarity: "dark", accent: "pine", prose_face: "system" });
    themeStore.loadConfig({ preset: "writ-light", overrides: {} });
    openThemeEditor();
    const select = container.querySelector<HTMLSelectElement>(".theme-editor-preset")!;
    expect(select.value).toBe("writ-dark");
    expect(select.selectedOptions[0].textContent).toBe("Writ Dark");
  });

  it("says the accent setting drives the accent tokens", () => {
    const { container } = render(() => <ThemeEditor />);
    openThemeEditor();
    const note = container.querySelector(".theme-editor-note");
    expect(note).not.toBeNull();
    expect(note!.textContent).toContain("Accent");
  });

  it("rows read as words, not token leaves", () => {
    const { container } = render(() => <ThemeEditor />);
    openThemeEditor();
    const names = [...container.querySelectorAll(".theme-editor-name")].map((n) => n.textContent);
    // The accent foreground row read "fg" beside a status row reading
    // "foreground", two names for one idea and neither a word a reader uses.
    expect(names).toContain("Text on accent");
    expect(names).toContain("Text on status");
    expect(names).not.toContain("fg");
    expect(names).not.toContain("foreground");
    expect(names).not.toContain("default");
  });

  it("labels the picker with the row's words", () => {
    const { container } = render(() => <ThemeEditor />);
    openThemeEditor();
    expect(pickerFor(container, "accent.fg").getAttribute("aria-label")).toBe("Text on accent");
  });

  it("drops the accent note on a preset that carries its own accent", () => {
    const { container } = render(() => <ThemeEditor />);
    themeStore.loadConfig({ preset: "tokyo-night", overrides: {} });
    openThemeEditor();
    expect(themeStore.accentApplies()).toBe(false);
    expect(container.querySelector(".theme-editor-note")).toBeNull();
  });
});
