import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

vi.mock("../../services/tauri", () => ({
  updateConfig: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  checkSpelling: vi.fn().mockResolvedValue([]),
  spellingAddIgnoredWord: vi.fn().mockResolvedValue(undefined),
}));

import SpellingChip from "../../components/Editor/SpellingChip";
import ContextMenu, { hideContextMenu } from "../../components/ContextMenu/ContextMenu";
import { spellingStore } from "../../stores/global/spelling";
import { configStore } from "../../stores/global/config";

async function setEnabled(enabled: boolean) {
  const current = configStore.config();
  await configStore.save({ ...current, spelling: { ...current.spelling, enabled } });
}

beforeEach(async () => {
  spellingStore.detach();
  await setEnabled(false);
});

afterEach(() => {
  hideContextMenu();
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
    await waitFor(() => expect(chip.textContent).toBe("3 spelling"));
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
    expect(queryByText("Preview…")).not.toBeNull();
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
    expect(queryByText("Preview…")).toBeNull();
  });
});
