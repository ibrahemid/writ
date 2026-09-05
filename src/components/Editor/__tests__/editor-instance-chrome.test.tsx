import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider from "../../WindowProvider/WindowProvider";

const bufferContent = new Map<string, string>();

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

function mockBuffer(id: string, filename: string, sizeBytes = 0): BufferDocument {
  return {
    id,
    title: filename,
    filename,
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: sizeBytes,
    line_ending: "lf",
  };
}

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(initial: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buf, setBuf] = createSignal(initial);
  const result = render(() => (
    <WindowProvider windowId={9201}>
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return { ...result, setBuf };
}

function hasGutter(container: HTMLElement): boolean {
  return container.querySelector(".cm-gutters") !== null;
}

function hasActiveLine(container: HTMLElement): boolean {
  return container.querySelector(".cm-activeLine") !== null;
}

// writCodeFace is the only rule that puts the mono token on the content, so
// the computed value says whether the face compartment carries it.
const MONO = "var(--writ-font-mono)";

function contentFace(container: HTMLElement): string {
  return getComputedStyle(container.querySelector(".cm-content")!).fontFamily;
}

describe("EditorInstance: gutter and active line follow the buffer's language", () => {
  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("opens a markdown note with no gutter and no active-line background", async () => {
    bufferContent.set("N", "# Heading\n\nSome prose.\n");
    const { container } = await mount(mockBuffer("N", "note.md"));

    expect(container.querySelectorAll(".cm-line").length).toBeGreaterThan(0);
    expect(hasGutter(container)).toBe(false);
    expect(hasActiveLine(container)).toBe(false);
  });

  it("opens a buffer with no detectable language as prose", async () => {
    bufferContent.set("S", "just some words\n");
    const { container } = await mount(mockBuffer("S", "scratch"));

    expect(container.querySelectorAll(".cm-line").length).toBeGreaterThan(0);
    expect(hasGutter(container)).toBe(false);
  });

  it("dresses the surface as code when a rename turns the note into a source file", async () => {
    // The real path: a title/filename change runs applyLanguageFromBuffer,
    // which re-detects and reconfigures the compartments in place.
    bufferContent.set("R", "fn main() {\n    let x = 1;\n}\n");
    const { container, setBuf } = await mount(mockBuffer("R", "note.md"));
    expect(hasGutter(container)).toBe(false);

    expect(contentFace(container)).not.toBe(MONO);

    setBuf(mockBuffer("R", "main.rs"));
    await flushMicrotasks();

    expect(hasGutter(container)).toBe(true);
    expect(container.querySelector(".cm-lineNumbers")).not.toBeNull();
    expect(hasActiveLine(container)).toBe(true);
    expect(contentFace(container)).toBe(MONO);
  });

  it("strips the surface back to prose when the source file is renamed to a note", async () => {
    bufferContent.set("R2", "fn main() {}\n");
    const { container, setBuf } = await mount(mockBuffer("R2", "main.rs"));
    expect(hasGutter(container)).toBe(true);
    expect(hasActiveLine(container)).toBe(true);
    expect(contentFace(container)).toBe(MONO);

    setBuf(mockBuffer("R2", "notes.md"));
    await flushMicrotasks();

    expect(hasGutter(container)).toBe(false);
    expect(hasActiveLine(container)).toBe(false);
    expect(contentFace(container)).not.toBe(MONO);
  });

  it("keeps the line numbers and the active line on a large-mode buffer", async () => {
    // A restricted buffer never has its language detected, so the prose
    // predicate alone would strip the numbers off an unwrapped file.
    bufferContent.set("BIG", "alpha\nbeta\ngamma\n");
    const { container } = await mount(mockBuffer("BIG", "huge.log", 6 * 1024 * 1024));

    expect(container.querySelectorAll(".cm-line").length).toBeGreaterThan(0);
    expect(hasGutter(container)).toBe(true);
    expect(container.querySelector(".cm-lineNumbers")).not.toBeNull();
    expect(hasActiveLine(container)).toBe(true);
    // It stays prose-faced: the gutter is for navigating, not a code dress.
    expect(contentFace(container)).not.toBe(MONO);
  });

  it("keeps the line numbers on a binary buffer", async () => {
    bufferContent.set("BIN", "binary\n");
    const buffer = { ...mockBuffer("BIN", "app.bin"), read_only: true };
    const { container } = await mount(buffer);

    expect(hasGutter(container)).toBe(true);
  });
});
