import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider from "../../WindowProvider/WindowProvider";

const bufferContent = new Map<string, string>();

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
  noteDiskState: vi.fn(async () => null),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

function mockBuffer(id: string, sizeBytes = 0): BufferDocument {
  return {
    id,
    title: id,
    filename: `${id}.md`,
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
  };
}

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(buffer: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const result = render(() => (
    <WindowProvider windowId={9101}>
      <EditorInstance buffer={buffer} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return result;
}

describe("EditorInstance: per-line automatic text direction", () => {
  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("marks each line dir=auto so Arabic and Latin lines resolve separately", async () => {
    bufferContent.set("AR", "مرحبا بالعالم\nconst greeting = 1;");
    const { container } = await mount(mockBuffer("AR"));

    const lines = Array.from(container.querySelectorAll(".cm-line"));
    expect(lines.length).toBeGreaterThan(0);
    for (const line of lines) {
      expect(line.getAttribute("dir")).toBe("auto");
    }
  });

  it("skips direction marking in the restricted large-file mode", async () => {
    bufferContent.set("BIG", "مرحبا بالعالم\nconst greeting = 1;");
    const { container } = await mount(mockBuffer("BIG", 6 * 1024 * 1024));

    expect(container.querySelectorAll(".cm-line").length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".cm-line[dir]").length).toBe(0);
  });
});
