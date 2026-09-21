import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { getCommand } from "../../commands/registry";
import { WIKILINK_CLASS } from "../../editor/wikilink-decorations";

// The Markdown extensions load for a Markdown file, keyed on the file's
// extension rather than on the detected language: a scratch buffer with no
// extension detects as markdown but is not a Markdown file.

const bufferContent = new Map<string, string>();

vi.mock("../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async () => () => {}),
  emitFrontendReady: vi.fn(async () => {}),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

const MD_SOURCE = "# Heading\n\nSee [[Other]] and https://example.com here.\n";

function mockBuffer(id: string, name: string, sourcePath: string | null): BufferDocument {
  return {
    id,
    title: name,
    filename: name,
    status: "active",
    language: null,
    source_path: sourcePath,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

async function flushMicrotasks(n = 20) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(buffer: BufferDocument) {
  const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
  const [buf] = createSignal(buffer);
  const result = render(() => (
    <WindowProvider windowId={9501}>
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return result;
}

describe("EditorInstance: the markdown extensions load for a markdown file", () => {
  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("loads the markdown extensions for a .md buffer", async () => {
    bufferContent.set("G1", MD_SOURCE);
    const { container } = await mount(mockBuffer("G1", "plan.md", "/files/plan.md"));

    expect(container.querySelector(".cm-line-md-h1")).not.toBeNull();
    expect(container.querySelector(`.${WIKILINK_CLASS}`)).not.toBeNull();
    expect(getCommand("editor.toggleInlineCode")).toBeDefined();
  });

  it("loads no markdown extension for a .txt buffer", async () => {
    bufferContent.set("G2", MD_SOURCE);
    const { container } = await mount(mockBuffer("G2", "plan.txt", "/files/plan.txt"));

    expect(container.querySelector(".cm-line-md-h1")).toBeNull();
    expect(container.querySelector(`.${WIKILINK_CLASS}`)).toBeNull();
  });

  it("loads no markdown extension for an untitled scratch buffer with no extension", async () => {
    bufferContent.set("G3", MD_SOURCE);
    const { container } = await mount(mockBuffer("G3", "untitled", null));

    expect(container.querySelector(".cm-line-md-h1")).toBeNull();
    expect(container.querySelector(`.${WIKILINK_CLASS}`)).toBeNull();
  });

  it("keeps the link layer on a .txt buffer", async () => {
    bufferContent.set("G4", MD_SOURCE);
    const { container } = await mount(mockBuffer("G4", "plan.txt", "/files/plan.txt"));

    expect(container.querySelector(".writ-link")).not.toBeNull();
  });

  it("registers no markdown format command on a .txt buffer", async () => {
    bufferContent.set("G5", MD_SOURCE);
    await mount(mockBuffer("G5", "plan.txt", "/files/plan.txt"));

    expect(getCommand("editor.toggleInlineCode")).toBeUndefined();
    expect(getCommand("editor.toggleBold")).toBeUndefined();
  });
});
