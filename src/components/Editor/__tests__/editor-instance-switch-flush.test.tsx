import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../../types/buffer";
import WindowProvider from "../../WindowProvider/WindowProvider";

const bufferContent = new Map<string, string>();
const callLog: string[] = [];

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async (id: string, content: string) => {
    bufferContent.set(id, content);
    callLog.push(`save:${id}:${content}`);
  }),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

function mockBuffer(id: string, title = id): BufferDocument {
  return {
    id,
    title,
    filename: `${title}.md`,
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
  };
}

async function flushMicrotasks(n = 3) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

describe("EditorInstance: buffer-switch flushes pending autosave (incl. empty)", () => {
  beforeEach(() => {
    bufferContent.clear();
    callLog.length = 0;
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("persists an intentional empty buffer on switch (no length guard)", async () => {
    const EditorInstance = (await import("../EditorInstance")).default;
    const { debouncedSave } = await import("../../../services/autosave");

    bufferContent.set("A", "previously saved");
    bufferContent.set("B", "");

    const [buf, setBuf] = createSignal(mockBuffer("A"));
    render(() => (
      <WindowProvider windowId={9001}>
        <EditorInstance buffer={buf()} />
      </WindowProvider>
    ));

    await flushMicrotasks(10);

    debouncedSave("A", "", 50_000);

    setBuf(mockBuffer("B"));
    await flushMicrotasks(20);

    expect(callLog).toContain("save:A:");
    expect(bufferContent.get("A")).toBe("");
  });

  it("persists pending non-empty content on switch", async () => {
    const EditorInstance = (await import("../EditorInstance")).default;
    const { debouncedSave } = await import("../../../services/autosave");

    bufferContent.set("X", "");
    bufferContent.set("Y", "");

    const [buf, setBuf] = createSignal(mockBuffer("X"));
    render(() => (
      <WindowProvider windowId={9001}>
        <EditorInstance buffer={buf()} />
      </WindowProvider>
    ));

    await flushMicrotasks(10);

    debouncedSave("X", "hello typed but timer not yet fired", 50_000);

    setBuf(mockBuffer("Y"));
    await flushMicrotasks(20);

    expect(callLog).toContain("save:X:hello typed but timer not yet fired");
    expect(bufferContent.get("X")).toBe("hello typed but timer not yet fired");
  });
});
