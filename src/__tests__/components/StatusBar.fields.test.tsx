import { describe, it, expect, afterEach, vi } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";

const [counts, setCounts] = createSignal(false);
const [buffer, setBuffer] = createSignal<BufferDocument | null>(null);
const [cursorLine, setCursorLine] = createSignal(1);
const [cursorCol, setCursorCol] = createSignal(1);

vi.mock("../../stores/global/save-status", () => ({
  saveStatusStore: { forNote: () => ({ state: "dirty" as const, fileName: "draft.txt" }) },
}));
vi.mock("../../stores/global/config", () => ({
  configStore: { config: () => ({ editor: { status_bar_counts: counts() } }) },
}));
vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: { getActive: () => null },
}));
vi.mock("../../stores/global/token-estimate", () => ({
  tokenEstimateStore: { count: () => 42, request: vi.fn() },
  formatTokenCount: (value: number) => String(value),
}));
vi.mock("../../stores/global/renderer-registry", () => ({
  rendererRegistry: { hasRenderer: (type: string | null) => type !== null },
}));
vi.mock("../../lib/use-active-buffer", () => ({ useActiveBuffer: () => buffer }));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({
    editor: {
      largeFileMode: () => null,
      cursorLine,
      cursorCol,
      isUpdatedFromDisk: () => false,
      currentText: () => "one two three",
    },
    tabs: { activeTabId: () => "tab-1" },
    layout: { get: () => ({ kind: "inline" as const }), set: vi.fn() },
  }),
}));

import StatusBar from "../../components/Editor/StatusBar";

function fileNamed(name: string): BufferDocument {
  return { id: "tab-1", title: name, filename: name, source_path: `/w/${name}` } as BufferDocument;
}

function right(container: HTMLElement): HTMLElement {
  return container.querySelector<HTMLElement>(".statusbar-right")!;
}

afterEach(() => {
  setCounts(false);
  setBuffer(null);
  setCursorLine(1);
  setCursorCol(1);
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
    expect(container.querySelector(".statusbar-live")!.textContent).toContain(
      "Unsaved changes in draft.txt",
    );
    expect(right(container).querySelector(".layout-toggle")).toBeNull();
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
    expect(right(container).querySelector(".statusbar-tokens")!.textContent).toContain("42 tokens");
  });

  it("carries the fields in reading order", () => {
    setCounts(true);
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    const classes = [...right(container).children].map((el) => el.className);
    expect(classes).toEqual([
      "statusbar-field statusbar-field--cursor",
      "statusbar-field statusbar-field--words",
      "statusbar-tokens",
      "statusbar-field",
      "layout-toggle",
    ]);
  });

  it("shows no folder button, language label, spelling chip, rewrite chip, scripts toggle or palette hint", () => {
    setCounts(true);
    setBuffer(fileNamed("draft.md"));
    const { container } = render(() => <StatusBar />);
    for (const selector of [".statusbar-folder", ".spelling-chip", ".scripts-toggle", ".kbd-chord"]) {
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
