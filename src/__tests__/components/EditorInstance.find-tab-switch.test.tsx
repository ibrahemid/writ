import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { findStore } from "../../stores/global/find-store";

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

function mockBuffer(id: string, name: string): BufferDocument {
  return {
    id,
    title: name,
    filename: name,
    status: "active",
    language: null,
    source_path: `/files/${name}`,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
  };
}

describe("EditorInstance: Find across a tab switch", () => {
  afterEach(() => {
    findStore.close();
    cleanup();
  });

  it("counts and highlights the file in front when Find stays open across the switch", async () => {
    bufferContent.set("F1", "needle and needle");
    bufferContent.set("F2", "one needle");
    const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
    const [buffer, setBuffer] = createSignal(mockBuffer("F1", "first.txt"));
    const { container } = render(() => (
      <WindowProvider windowId={9601}>
        <EditorInstance buffer={buffer()} />
      </WindowProvider>
    ));
    await vi.waitFor(() => expect(container.querySelector(".cm-content")?.textContent).toBe("needle and needle"));

    findStore.open();
    findStore.setQueryText("needle");
    await vi.waitFor(() => expect(findStore.matches().total).toBe(2));

    setBuffer(mockBuffer("F2", "second.txt"));
    await vi.waitFor(() => expect(container.querySelector(".cm-content")?.textContent).toBe("one needle"));
    await vi.waitFor(() => expect(findStore.matches().total).toBe(1));
    expect(container.querySelectorAll(".cm-searchMatch")).toHaveLength(1);
  });
});
