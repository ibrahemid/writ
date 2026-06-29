import { describe, it, expect, vi, afterEach, beforeEach, beforeAll } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";

beforeAll(() => {
  if (!(Element.prototype as { scrollIntoView?: () => void }).scrollIntoView) {
    Element.prototype.scrollIntoView = function scrollIntoView() {};
  }
});

const h = vi.hoisted(() => ({
  usage: {} as Record<string, unknown>,
  recordCommandUse: vi.fn(),
  openSettings: vi.fn(),
}));

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ commands: { usage: h.usage } }),
    recordCommandUse: h.recordCommandUse,
  },
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { focusEditor: vi.fn() },
  }),
}));

vi.mock("../../components/SettingsModal/SettingsModal", () => ({
  openSettings: h.openSettings,
  default: () => null,
}));

vi.mock("../../settings/availability", () => ({
  isSettingAvailable: () => true,
}));

import CommandPalette, {
  openCommandPalette,
  closeCommandPalette,
} from "../../components/CommandPalette/CommandPalette";
import { registerCommand, getAllCommands, unregisterCommand } from "../../commands/registry";

function sectionLabels(container: Element): string[] {
  return Array.from(container.querySelectorAll(".palette-section-label")).map(
    (el) => el.textContent ?? "",
  );
}

describe("CommandPalette settings results", () => {
  beforeEach(() => {
    for (const cmd of [...getAllCommands()]) unregisterCommand(cmd.id);
    h.recordCommandUse.mockClear();
    h.openSettings.mockClear();
    registerCommand({
      id: "cmd.alpha",
      label: "Alpha command",
      scope: "app",
      execute: vi.fn(),
    });
    registerCommand({
      id: "cmd.font",
      label: "Font command",
      scope: "app",
      execute: vi.fn(),
    });
  });

  afterEach(() => {
    closeCommandPalette();
    cleanup();
  });

  it("shows no settings section on an empty query", async () => {
    const { container } = render(() => <CommandPalette />);
    openCommandPalette();
    await waitFor(() => expect(container.querySelector(".palette")).not.toBeNull());
    expect(sectionLabels(container)).not.toContain("Settings");
  });

  it("appends a Settings section below command results when a query matches", async () => {
    const user = userEvent.setup();
    const { container } = render(() => <CommandPalette />);
    openCommandPalette();
    await waitFor(() => expect(container.querySelector(".palette-input")).not.toBeNull());
    await user.type(container.querySelector(".palette-input")!, "font");
    await waitFor(() => {
      const settingsSection = container.querySelector(".palette-section-settings");
      expect(settingsSection).not.toBeNull();
      expect(settingsSection!.textContent).toContain("Font size");
    });
    // Settings always render after the command results section.
    const sections = Array.from(container.querySelectorAll(".palette-section"));
    const settingsIdx = sections.findIndex((s) => s.classList.contains("palette-section-settings"));
    const resultsIdx = sections.findIndex((s) => s.classList.contains("palette-section-results"));
    expect(resultsIdx).toBeGreaterThanOrEqual(0);
    expect(settingsIdx).toBeGreaterThan(resultsIdx);
  });

  it("shows settings even when no command matches", async () => {
    const user = userEvent.setup();
    const { container } = render(() => <CommandPalette />);
    openCommandPalette();
    await waitFor(() => expect(container.querySelector(".palette-input")).not.toBeNull());
    await user.type(container.querySelector(".palette-input")!, "autosave");
    await waitFor(() => {
      expect(container.querySelector(".palette-section-results")).toBeNull();
      expect(container.querySelector(".palette-section-settings")).not.toBeNull();
    });
  });

  it("opens settings deep-linked and does not record usage when a setting is chosen", async () => {
    const user = userEvent.setup();
    const { container } = render(() => <CommandPalette />);
    openCommandPalette();
    await waitFor(() => expect(container.querySelector(".palette-input")).not.toBeNull());
    await user.type(container.querySelector(".palette-input")!, "autosave");
    await waitFor(() => expect(container.querySelector(".palette-section-settings")).not.toBeNull());
    const item = container.querySelector<HTMLButtonElement>(
      ".palette-section-settings .palette-item",
    );
    expect(item).not.toBeNull();
    await user.click(item!);
    expect(h.openSettings).toHaveBeenCalledWith("files", "files.autosave");
    expect(h.recordCommandUse).not.toHaveBeenCalled();
  });
});
