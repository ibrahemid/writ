import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { EditorView } from "@codemirror/view";
import type { BufferDocument } from "../../types/buffer";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { configStore } from "../../stores/global/config";

const bufferContent = new Map<string, string>();

vi.mock("../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
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

describe("EditorInstance: font zoom re-measures the view", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    cleanup();
  });

  it("asks CodeMirror to measure in the same tick the font size changes", async () => {
    bufferContent.set("Z1", "a line under the cursor");
    const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
    const { container } = render(() => (
      <WindowProvider windowId={9701}>
        <EditorInstance buffer={mockBuffer("Z1", "zoom.txt")} />
      </WindowProvider>
    ));
    await vi.waitFor(() => expect(container.querySelector(".cm-content")?.textContent).toBe("a line under the cursor"));

    const before = configStore.config().editor.font_size;
    const requestMeasure = vi.spyOn(EditorView.prototype, "requestMeasure");
    configStore.setEditorFontSize(before + 1);
    expect(requestMeasure).toHaveBeenCalled();

    requestMeasure.mockClear();
    configStore.setEditorFontSize(before + 1);
    expect(requestMeasure).not.toHaveBeenCalled();

    configStore.setEditorFontSize(before);
  });
});
