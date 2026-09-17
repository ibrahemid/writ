import { describe, it, expect, afterEach, vi } from "vitest";
import { render, fireEvent, cleanup, screen } from "@solidjs/testing-library";

vi.mock("../../services/tauri", () => ({
  checkSpelling: vi.fn().mockResolvedValue([]),
  spellingAddIgnoredWord: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}));

import SpellingPreview, {
  openSpellingPreview,
  closeSpellingPreview,
} from "../../components/Editor/SpellingPreview";

afterEach(() => {
  closeSpellingPreview();
  cleanup();
});

// A click on a button does not focus it on macOS, so after the chip's menu item
// runs, the active element is the body and a keydown never bubbles through the
// panel. The panel that says it is a dialog has to hold the focus itself.
describe("SpellingPreview focus", () => {
  it("takes focus on its first live action when it opens", async () => {
    render(() => <SpellingPreview />);
    openSpellingPreview();
    await screen.findByText("Apply");
    const panel = document.querySelector(".spelling-preview")!;
    expect(panel.contains(document.activeElement)).toBe(true);
    expect((document.activeElement as HTMLButtonElement).disabled).toBe(false);
  });

  it("closes on Escape pressed with the focus outside it", async () => {
    render(() => <SpellingPreview />);
    openSpellingPreview();
    await screen.findByText("Apply");

    document.body.focus();
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(document.querySelector(".spelling-preview")).toBeNull();
  });

  it("closes on Escape pressed inside it", async () => {
    render(() => <SpellingPreview />);
    openSpellingPreview();
    const apply = await screen.findByText("Apply");

    fireEvent.keyDown(apply, { key: "Escape" });
    expect(document.querySelector(".spelling-preview")).toBeNull();
  });

  it("gives the focus back where it came from", async () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();

    render(() => <SpellingPreview />);
    openSpellingPreview();
    await screen.findByText("Apply");
    closeSpellingPreview();

    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it("declares itself modal", async () => {
    render(() => <SpellingPreview />);
    openSpellingPreview();
    await screen.findByText("Apply");
    expect(
      document.querySelector(".spelling-preview")?.getAttribute("aria-modal"),
    ).toBe("true");
  });
});
