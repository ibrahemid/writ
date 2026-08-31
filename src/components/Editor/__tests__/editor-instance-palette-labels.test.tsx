import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider from "../../WindowProvider/WindowProvider";
import { getCommand } from "../../../commands/registry";

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
  };
}

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(buffer: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  return render(() => (
    <WindowProvider windowId={9301}>
      <EditorInstance buffer={buffer} />
    </WindowProvider>
  )).container;
}

describe("EditorInstance: palette labels for the formatting trio", () => {
  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("names each format the mark it applies", async () => {
    bufferContent.set("F", "# Heading\n\nSome prose.\n");
    await mount(mockBuffer("F", "note.md"));
    await flushMicrotasks();

    expect(getCommand("editor.toggleBold")?.label).toBe("Bold");
    expect(getCommand("editor.toggleItalic")?.label).toBe("Italic");
    expect(getCommand("editor.toggleInlineCode")?.label).toBe("Inline code");
  });
});
