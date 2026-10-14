import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";

const [counts, setCounts] = createSignal(false);
const [buffer, setBuffer] = createSignal<BufferDocument | null>(null);
const [cursorLine, setCursorLine] = createSignal(1);
const [cursorCol, setCursorCol] = createSignal(1);
const [spellingOn, setSpellingOn] = createSignal(false);
const [flagged, setFlagged] = createSignal(0);

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { forNote: () => ({ state: "saved" as const, fileName: "draft.txt" }) },
}));
vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ editor: { status_bar_counts: counts() }, spelling: { enabled: spellingOn() } }),
  },
}));
vi.mock("../../stores/global/spelling", () => ({ spellingStore: { count: () => flagged() } }));
const openSpellingPreview = vi.hoisted(() => vi.fn());
vi.mock("../../components/Editor/SpellingPreview", () => ({ openSpellingPreview }));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => null },
}));
vi.mock("../../stores/global/renderer-registry", () => ({
  rendererRegistry: { hasRenderer: (type: string | null) => type !== null },
}));
vi.mock("../../lib/use-active-buffer", () => ({ useActiveBuffer: () => buffer }));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: {
      largeFileMode: () => null,
      currentBufferId: () => "tab-1",
      tierOffer: () => null,
      longestLine: () => 0,
      overridesOf: () => ({}),
      cursorLine,
      cursorCol,
      isUpdatedFromDisk: () => false,
    },
    counts: { counts: () => ({ words: 3, characters: 13, cjk: 0, arabic: 0, exact: true }), watch: () => () => {} },
    tabs: { activeTabId: () => "tab-1" },
    layout: { get: () => ({ kind: "inline" as const }), set: vi.fn() },
  }),
}));

import StatusBar from "../../components/Editor/StatusBar";

const openEncodingPicker = vi.hoisted(() => vi.fn());
vi.mock("../../components/Editor/EncodingPicker", () => ({ openEncodingPicker }));

function fileNamed(name: string, encoding: string | null = "UTF-8"): BufferDocument {
  return {
    id: "tab-1",
    title: name,
    filename: name,
    source_path: `/w/${name}`,
    encoding,
  } as BufferDocument;
}

function right(container: HTMLElement): HTMLElement {
  return container.querySelector<HTMLElement>(".statusbar-right")!;
}

afterEach(() => {
  setCounts(false);
  setBuffer(null);
  setCursorLine(1);
  setCursorCol(1);
  setSpellingOn(false);
  setFlagged(0);
  openSpellingPreview.mockClear();
  cleanup();
});

describe("StatusBar fields", () => {
  it("shows line, column, encoding and save state for a .txt buffer", () => {
    setBuffer(fileNamed("draft.txt"));
    setCursorLine(12);
    setCursorCol(4);
    const { container } = render(() => <StatusBar />);
    expect(right(container).textContent).toContain("Ln 12, Col 4");
    expect(right(container).textContent).toContain("UTF-8");
    expect(container.querySelector(".statusbar-live")!.textContent).toContain("Saved draft.txt");
    expect(right(container).querySelector(".layout-toggle")).toBeNull();
  });

  it("shows the file's own encoding as the control that reopens it in another", () => {
    setBuffer(fileNamed("prices.csv", "windows-1252"));
    const { container } = render(() => <StatusBar />);
    const control = right(container).querySelector<HTMLButtonElement>(".statusbar-encoding")!;
    expect(control.textContent).toBe("Windows-1252");
    expect(right(container).textContent).not.toContain("UTF-8");
    control.click();
    expect(openEncodingPicker).toHaveBeenCalledTimes(1);
  });

  it("names a UTF-8 byte order mark and UTF-16's byte order", () => {
    setBuffer(fileNamed("notes.txt", "UTF-8 BOM"));
    const { container, unmount } = render(() => <StatusBar />);
    expect(right(container).querySelector(".statusbar-encoding")!.textContent).toBe(
      "UTF-8 with BOM",
    );
    unmount();
    setBuffer(fileNamed("out.txt", "UTF-16LE BOM"));
    const second = render(() => <StatusBar />);
    expect(right(second.container).querySelector(".statusbar-encoding")!.textContent).toBe(
      "UTF-16 LE",
    );
  });

  it("shows a note with no file yet as UTF-8, with nothing to reopen", () => {
    setBuffer({ ...fileNamed("draft.txt"), source_path: null });
    const { container } = render(() => <StatusBar />);
    expect(right(container).querySelector(".statusbar-encoding")).toBeNull();
    expect(right(container).textContent).toContain("UTF-8");
  });

  it("shows no encoding for a file that has none", () => {
    setBuffer(fileNamed("blob.bin", null));
    const { container } = render(() => <StatusBar />);
    expect(right(container).querySelector(".statusbar-encoding")).toBeNull();
    expect(right(container).textContent).not.toContain("UTF-8");
  });

  it("adds the mode control for a .md buffer", () => {
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    const toggle = right(container).querySelector(".layout-toggle")!;
    expect(toggle).not.toBeNull();
    expect([...toggle.querySelectorAll(".layout-toggle-label")].map((el) => el.textContent)).toEqual(
      ["Inline", "Source"],
    );
  });

  it("hides the counts unless the counts switch is on", () => {
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    expect(right(container).querySelector(".statusbar-field--words")).toBeNull();
    expect(right(container).querySelector(".statusbar-tokens")).toBeNull();
  });

  it("shows word, character and token counts when the counts switch is on", () => {
    setCounts(true);
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    expect(right(container).querySelector(".statusbar-field--words")!.textContent).toBe(
      "3 words, 13 characters",
    );
    expect(right(container).querySelector(".statusbar-tokens")!.textContent).toBe("≈ 4 tokens");
  });

  it("carries the fields in reading order", () => {
    setCounts(true);
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    const classes = [
      ...right(container).querySelectorAll(
        ".statusbar-field--cursor, .statusbar-field--words, .statusbar-tokens, .statusbar-encoding, .layout-toggle",
      ),
    ].map((el) => el.className);
    expect(classes).toEqual([
      "statusbar-field statusbar-field--cursor",
      "statusbar-field statusbar-field--words",
      "statusbar-tokens",
      "statusbar-field statusbar-encoding",
      "layout-toggle",
    ]);
  });

  it("shows no spelling chip while spell check is off, whatever it counted", () => {
    setFlagged(4);
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    expect(container.querySelector(".statusbar-chip--spelling")).toBeNull();
  });

  it("shows no spelling chip while spell check is on and nothing is flagged", () => {
    setSpellingOn(true);
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    expect(container.querySelector(".statusbar-chip--spelling")).toBeNull();
  });

  it("counts what spell check flagged and opens the review from the chip", () => {
    setSpellingOn(true);
    setFlagged(14);
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    const chip = container.querySelector<HTMLButtonElement>(".statusbar-chip--spelling")!;
    expect(chip.textContent).toBe("14 flagged");
    chip.click();
    expect(openSpellingPreview).toHaveBeenCalledTimes(1);

    setFlagged(0);
    expect(container.querySelector(".statusbar-chip--spelling")).toBeNull();
  });

  it("shows no folder button, language label, rewrite chip, scripts toggle or palette hint", () => {
    setCounts(true);
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    for (const selector of [".statusbar-folder", ".scripts-toggle", ".kbd-chord"]) {
      expect(container.querySelector(selector), selector).toBeNull();
    }
    for (const word of ["Rewrite", "Command palette", "Markdown", "Plain Text"]) {
      expect(container.textContent, word).not.toContain(word);
    }
  });

  // The bar's own vocabulary: sentence case, and no em dash, which UI copy
  // never carries.
  it("writes every label in sentence case", () => {
    const { container } = render(() => <StatusBar />);
    const labels = [...container.querySelectorAll(".statusbar-label")].map(
      (el) => el.textContent ?? "",
    );
    expect(labels.length).toBeGreaterThan(0);
    for (const label of labels) {
      expect(label[0], label).toBe(label[0].toUpperCase());
      expect(label, label).not.toContain("—");
    }
  });
});
