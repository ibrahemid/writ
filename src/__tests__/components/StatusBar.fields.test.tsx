import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { status: () => "idle" as const },
}));
vi.mock("../../commands/registry", () => ({ useCommand: () => undefined }));
vi.mock("../../commands/keybindings", () => ({ useEffectiveBinding: () => null }));
vi.mock("../../components/Kbd/Kbd", () => ({ default: () => null }));
vi.mock("../../components/Editor/TokenEstimate", () => ({ default: () => null }));
vi.mock("../../components/Preview/PreviewLayoutToggle", () => ({ default: () => null }));
vi.mock("../../components/Preview/PreviewScriptsToggle", () => ({ default: () => null }));

const [cursorLine, setCursorLine] = createSignal(1);
const [cursorCol, setCursorCol] = createSignal(1);
const [language, setLanguage] = createSignal<string | null>(null);

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: { largeFileMode: () => null, cursorLine, cursorCol, language },
  }),
}));

import StatusBar from "../../components/Editor/StatusBar";

describe("StatusBar document fields", () => {
  afterEach(() => {
    setCursorLine(1);
    setCursorCol(1);
    setLanguage(null);
    cleanup();
  });

  it("renders language label, encoding, and cursor position", () => {
    setLanguage("markdown");
    setCursorLine(12);
    setCursorCol(4);
    const { container } = render(() => <StatusBar />);
    const text = container.querySelector(".statusbar-right")!.textContent ?? "";
    expect(text).toContain("Markdown");
    expect(text).toContain("UTF-8");
    expect(text).toContain("Ln 12, Col 4");
  });

  it("shows Plain Text when no language is detected", () => {
    setLanguage(null);
    const { container } = render(() => <StatusBar />);
    expect(container.querySelector(".statusbar-right")!.textContent).toContain("Plain Text");
  });

  it("tracks cursor movement reactively", () => {
    const { container } = render(() => <StatusBar />);
    setCursorLine(99);
    setCursorCol(7);
    expect(container.querySelector(".statusbar-right")!.textContent).toContain("Ln 99, Col 7");
  });
});
