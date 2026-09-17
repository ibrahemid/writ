import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

vi.mock("../../services/tauri", () => ({
  updateConfig: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  checkSpelling: vi.fn().mockResolvedValue([]),
  spellingAddIgnoredWord: vi.fn().mockResolvedValue(undefined),
}));

const settingsMocks = vi.hoisted(() => ({ openSettings: vi.fn() }));

vi.mock("../../components/SettingsModal/SettingsModal", () => ({
  default: () => null,
  openSettings: settingsMocks.openSettings,
}));

import SpellingChip from "../../components/Editor/SpellingChip";
import ContextMenu, { hideContextMenu } from "../../components/ContextMenu/ContextMenu";
import SpellingPreview, { closeSpellingPreview } from "../../components/Editor/SpellingPreview";
import { spellingStore } from "../../stores/global/spelling";
import { configStore } from "../../stores/global/config";

async function setEnabled(enabled: boolean) {
  const current = configStore.config();
  await configStore.save({ ...current, spelling: { ...current.spelling, enabled } });
}

beforeEach(async () => {
  spellingStore.detach();
  settingsMocks.openSettings.mockClear();
  await setEnabled(false);
});

afterEach(() => {
  hideContextMenu();
  closeSpellingPreview();
  cleanup();
});

describe("SpellingChip visibility and states", () => {
  it("is hidden for an ineligible buffer", () => {
    spellingStore.setEligible(false);
    const { container } = render(() => <SpellingChip />);
    expect(container.querySelector(".spelling-chip")).toBeNull();
  });

  it("shows the muted off state when eligible and disabled", () => {
    spellingStore.setEligible(true);
    const { container } = render(() => <SpellingChip />);
    const chip = container.querySelector(".spelling-chip");
    expect(chip).not.toBeNull();
    expect(chip!.classList.contains("spelling-chip--off")).toBe(true);
    expect(chip!.textContent).toBe("Spelling off");
  });

  it("shows the plain label when on with no issues", async () => {
    spellingStore.setEligible(true);
    await setEnabled(true);
    spellingStore.publishCount(0);
    const { container } = render(() => <SpellingChip />);
    const chip = container.querySelector(".spelling-chip")!;
    await waitFor(() => expect(chip.textContent).toBe("Spelling"));
    expect(chip.classList.contains("spelling-chip--off")).toBe(false);
  });

  it("shows the count when on with issues", async () => {
    spellingStore.setEligible(true);
    await setEnabled(true);
    spellingStore.publishCount(3);
    const { container } = render(() => <SpellingChip />);
    const chip = container.querySelector(".spelling-chip")!;
    await waitFor(() => expect(chip.textContent).toBe("3 misspelled"));
  });

  it("reads as English at one and at three", async () => {
    spellingStore.setEligible(true);
    await setEnabled(true);
    spellingStore.publishCount(1);
    const { container } = render(() => <SpellingChip />);
    const chip = container.querySelector(".spelling-chip")!;
    await waitFor(() => expect(chip.textContent).toBe("1 misspelled"));
    spellingStore.publishCount(3);
    await waitFor(() => expect(chip.textContent).toBe("3 misspelled"));
  });
});

// A plain button gives no warning that pressing it opens a layer; the chip has
// to say that it does, and whether the layer is up.
describe("SpellingChip announces its menu", () => {
  it("says it opens a menu, and says when that menu is up", async () => {
    spellingStore.setEligible(true);
    const { container } = render(() => (
      <>
        <SpellingChip />
        <ContextMenu />
      </>
    ));

    const chip = container.querySelector<HTMLButtonElement>(".spelling-chip")!;
    expect(chip.getAttribute("aria-haspopup")).toBe("menu");
    expect(chip.getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(chip);
    await waitFor(() => expect(chip.getAttribute("aria-expanded")).toBe("true"));

    hideContextMenu();
    await waitFor(() => expect(chip.getAttribute("aria-expanded")).toBe("false"));
  });

  // A keyboard user is already on the chip when the menu opens, and the menu
  // does not take the focus until an arrow key moves into it. The dismiss then
  // refocuses an element that never lost the focus, so no focus event fires.
  it("clears the state when the menu is dismissed with Escape from the keyboard", async () => {
    spellingStore.setEligible(true);
    const { container } = render(() => (
      <>
        <SpellingChip />
        <ContextMenu />
      </>
    ));

    const chip = container.querySelector<HTMLButtonElement>(".spelling-chip")!;
    chip.focus();
    fireEvent.click(chip);
    await waitFor(() => expect(chip.getAttribute("aria-expanded")).toBe("true"));

    fireEvent.keyDown(chip, { key: "Escape" });
    await waitFor(() => expect(document.querySelector(".context-menu")).toBeNull());
    expect(chip.getAttribute("aria-expanded")).toBe("false");
  });
});

describe("SpellingChip menu-driven toggle", () => {
  it("opens a menu with Turn on spelling when off, and enabling persists", async () => {
    spellingStore.setEligible(true);
    const { container, getByText } = render(() => (
      <>
        <SpellingChip />
        <ContextMenu />
      </>
    ));

    fireEvent.click(container.querySelector(".spelling-chip")!);
    const turnOn = getByText("Turn on spelling");
    expect(turnOn).not.toBeNull();

    fireEvent.click(turnOn);
    await waitFor(() => expect(configStore.config().spelling.enabled).toBe(true));
  });

  it("offers Turn off spelling and the fix rows when on with issues", async () => {
    spellingStore.setEligible(true);
    await setEnabled(true);
    spellingStore.publishCount(2);
    const { container, getByText, queryByText } = render(() => (
      <>
        <SpellingChip />
        <ContextMenu />
      </>
    ));

    fireEvent.click(container.querySelector(".spelling-chip")!);
    expect(getByText("Turn off spelling")).not.toBeNull();
    expect(getByText("Fix all (2)")).not.toBeNull();
    expect(queryByText("Review fixes…")).not.toBeNull();
  });

  // The way a user actually opens the panel. The menu used to run the row and
  // then restore the focus to the chip, which left the panel with the ring
  // behind it and no keyboard way out.
  it("hands the focus to the spelling panel, which Escape then closes", async () => {
    spellingStore.setEligible(true);
    await setEnabled(true);
    spellingStore.publishCount(2);
    const { container, getByText } = render(() => (
      <>
        <SpellingChip />
        <ContextMenu />
        <SpellingPreview />
      </>
    ));

    fireEvent.click(container.querySelector(".spelling-chip")!);
    fireEvent.click(getByText("Review fixes…"));

    const panel = await waitFor(() => {
      const el = document.querySelector(".spelling-preview");
      expect(el).not.toBeNull();
      return el!;
    });
    expect(panel.contains(document.activeElement)).toBe(true);

    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(document.querySelector(".spelling-preview")).toBeNull();
  });

  // Reported: "spelling settings does not open from status bar when clicked".
  // The row carries `separator: true` to open its group, which the menu used to
  // read as "this row is a divider" and refuse to activate.
  it("opens spelling settings from the menu", async () => {
    spellingStore.setEligible(true);
    await setEnabled(true);
    const { container, getByText } = render(() => (
      <>
        <SpellingChip />
        <ContextMenu />
      </>
    ));

    fireEvent.click(container.querySelector(".spelling-chip")!);
    fireEvent.click(getByText("Spelling settings"));
    expect(settingsMocks.openSettings).toHaveBeenCalledWith("editor", "editor.spelling");
  });

  it("opens spelling settings while the feature is off", async () => {
    spellingStore.setEligible(true);
    const { container, getByText } = render(() => (
      <>
        <SpellingChip />
        <ContextMenu />
      </>
    ));

    fireEvent.click(container.querySelector(".spelling-chip")!);
    fireEvent.click(getByText("Spelling settings"));
    expect(settingsMocks.openSettings).toHaveBeenCalledWith("editor", "editor.spelling");
  });

  it("hides fix rows when on with no issues", async () => {
    spellingStore.setEligible(true);
    await setEnabled(true);
    spellingStore.publishCount(0);
    const { container, getByText, queryByText } = render(() => (
      <>
        <SpellingChip />
        <ContextMenu />
      </>
    ));

    fireEvent.click(container.querySelector(".spelling-chip")!);
    expect(getByText("Turn off spelling")).not.toBeNull();
    expect(queryByText(/^Fix all/)).toBeNull();
    expect(queryByText("Review fixes…")).toBeNull();
  });
});
