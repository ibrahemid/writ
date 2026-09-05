import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider from "../../WindowProvider/WindowProvider";
import { getAllCommands, getCommand } from "../../../commands/registry";

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

// A toggle names the mark it applies; an insertion keeps its verb. Both read as
// a sentence. A new formatting command has to be written in here to pass.
const FORMAT_LABELS: readonly (readonly [string, string])[] = [
  ["editor.toggleBold", "Bold"],
  ["editor.toggleItalic", "Italic"],
  ["editor.toggleStrikethrough", "Strikethrough"],
  ["editor.toggleInlineCode", "Inline code"],
  ["editor.insertLink", "Insert link"],
  ["editor.toggleBulletList", "Bulleted list"],
  ["editor.toggleTaskList", "Task list"],
];

function mockBuffer(id: string, filename: string): BufferDocument {
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
    size_bytes: 0,
    line_ending: "lf",
  };
}

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(initial: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buf, setBuf] = createSignal(initial);
  render(() => (
    <WindowProvider windowId={9301}>
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return setBuf;
}

function editorCommandIds(): Set<string> {
  return new Set(
    getAllCommands()
      .map((c) => c.id)
      .filter((id) => id.startsWith("editor.")),
  );
}

describe("EditorInstance: the palette's formatting labels", () => {
  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("names every formatting command in the table's shape", async () => {
    bufferContent.set("F", "# Heading\n\nSome prose.\n");
    await mount(mockBuffer("F", "note.md"));

    for (const [id, label] of FORMAT_LABELS) {
      expect(getCommand(id)?.label, id).toBe(label);
    }
  });

  it("registers no formatting command the table does not name", async () => {
    // A markdown buffer carries the group; a source file does not, so the
    // difference between the two registries is exactly the group.
    bufferContent.set("D", "# Heading\n\nSome prose.\n");
    const setBuf = await mount(mockBuffer("D", "note.md"));
    const withFormatting = editorCommandIds();

    setBuf(mockBuffer("D", "main.rs"));
    await flushMicrotasks();
    const withoutFormatting = editorCommandIds();

    const group = [...withFormatting].filter((id) => !withoutFormatting.has(id));
    expect(group.sort()).toEqual(FORMAT_LABELS.map(([id]) => id).sort());
  });
});
