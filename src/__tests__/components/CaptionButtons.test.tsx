import { describe, it, expect, afterEach, beforeEach, vi } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const h = vi.hoisted(() => ({ chord: "", taken: false }));

vi.mock("../../stores/global/hotkey", () => ({
  SHIPPED_WINDOW_CHORD: "CmdOrCtrl+Shift+Space",
  SHIPPED_CAPTURE_CHORD: "CmdOrCtrl+Shift+Alt+Space",
  hotkeyStore: { chord: () => h.chord, isTaken: () => h.taken },
}));

import CaptionButtons from "../../components/TitleBar/CaptionButtons";
import { formatKeybinding } from "../../lib/keybinding-format";

beforeEach(() => {
  h.chord = "CmdOrCtrl+Shift+Space";
  h.taken = false;
});

afterEach(() => {
  vi.restoreAllMocks();
  cleanup();
});

// jsdom answers `:focus-visible` false for every element, so a keyboard focus
// is stated rather than performed.
function focusByKeyboard(target: HTMLElement) {
  const real = Element.prototype.matches;
  vi.spyOn(Element.prototype, "matches").mockImplementation(function (this: Element, selector: string) {
    return selector === ":focus-visible" ? true : real.call(this, selector);
  });
  fireEvent.focusIn(target);
}

async function hint(kind: "win" | "linux-close"): Promise<string | null | undefined> {
  const { container } = render(() => <CaptionButtons kind={kind} maximized={false} />);
  focusByKeyboard(container.querySelector<HTMLButtonElement>("button[aria-label='Hide']")!);
  return waitFor(() => {
    const tip = document.querySelector("[role='tooltip']");
    expect(tip).not.toBeNull();
    return tip!.textContent;
  });
}

describe("the caption's hide button", () => {
  for (const kind of ["win", "linux-close"] as const) {
    it(`says on ${kind} that Writ keeps running and which chord brings it back`, async () => {
      expect(await hint(kind)).toBe(
        `Writ keeps running. ${formatKeybinding("CmdOrCtrl+Shift+Space")} brings it back.`,
      );
    });
  }

  it("names no chord another app holds", async () => {
    h.taken = true;
    expect(await hint("win")).toBe("Writ keeps running in the background.");
  });
});
