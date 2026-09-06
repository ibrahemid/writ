import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const h = vi.hoisted(() => ({ showInFileManager: vi.fn().mockResolvedValue(undefined) }));

vi.mock("../../stores/global/notes", () => ({
  notesStore: { showInFileManager: h.showInFileManager },
}));
vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { forNote: () => ({ state: "clean" as const, fileName: "note.md" }) },
}));
vi.mock("../../commands/registry", () => ({ useCommand: () => undefined }));
vi.mock("../../commands/keybindings", () => ({ useEffectiveBinding: () => null }));
vi.mock("../../components/Kbd/Kbd", () => ({ default: () => null }));
vi.mock("../../components/Editor/TokenEstimate", () => ({ default: () => null }));
vi.mock("../../components/Preview/PreviewLayoutToggle", () => ({ default: () => null }));
vi.mock("../../components/Preview/PreviewScriptsToggle", () => ({ default: () => null }));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: {
      largeFileMode: () => null,
      cursorLine: () => 1,
      cursorCol: () => 1,
      language: () => null,
      currentText: () => "",
      isUpdatedFromDisk: () => false,
    },
    tabs: { activeTabId: () => null },
  }),
}));

import StatusBar from "../../components/Editor/StatusBar";

afterEach(() => {
  h.showInFileManager.mockClear();
  cleanup();
});

describe("the status bar's way to the notes folder", () => {
  it("names the folder and opens it when clicked", () => {
    const { container } = render(() => <StatusBar />);
    const target = container.querySelector<HTMLButtonElement>(".statusbar-folder")!;
    expect(target.textContent).toBe("Notes");
    expect(target.title).toBe("Open the notes folder");

    fireEvent.click(target);
    expect(h.showInFileManager).toHaveBeenCalledTimes(1);
  });
});
