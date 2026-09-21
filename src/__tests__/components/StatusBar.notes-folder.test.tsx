import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";

const h = vi.hoisted(() => ({
  showInFileManager: vi.fn().mockResolvedValue(undefined),
  folderPath: { current: "/Users/x/Notes" as string | null },
}));

vi.mock("../../stores/global/notes", () => ({
  notesStore: {
    showInFileManager: h.showInFileManager,
    folder: () => (h.folderPath.current === null ? null : { path: h.folderPath.current }),
    root: () => h.folderPath.current,
  },
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

    fireEvent.click(target);
    expect(h.showInFileManager).toHaveBeenCalledTimes(1);
  });

  // The folder moves from Settings, and the word on the button is the folder's
  // own name, the way the sidebar heads a folder with its basename.
  it("reads the folder the notes are actually in", () => {
    h.folderPath.current = "/Users/x/Documents/Notebook";
    const { container } = render(() => <StatusBar />);
    expect(container.querySelector(".statusbar-folder")!.textContent).toBe("Notebook");
    h.folderPath.current = "/Users/x/Notes";
  });

  it("falls back to the word Files when no folder is loaded yet", () => {
    h.folderPath.current = null;
    const { container } = render(() => <StatusBar />);
    expect(container.querySelector(".statusbar-folder")!.textContent).toBe("Files");
    h.folderPath.current = "/Users/x/Notes";
  });

  // The content wins the accessible name, so the noun on the button is all a
  // screen reader or voice control gets unless the action is named.
  it("names the action, in the platform's own word for its file manager", () => {
    const { container } = render(() => <StatusBar />);
    const target = container.querySelector<HTMLButtonElement>(".statusbar-folder")!;
    expect(target.getAttribute("aria-label")).toMatch(/^Open Notes in \w+$/);
    expect(target.title).toBe(target.getAttribute("aria-label"));
  });
});
