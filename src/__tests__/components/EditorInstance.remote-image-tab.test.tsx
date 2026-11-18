import { describe, it, expect, vi, beforeAll, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import { loadGrammar } from "../../editor/languages";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";

const mocks = vi.hoisted(() => ({
  bufferContent: new Map<string, string>(),
  heldReads: new Map<string, Promise<string>>(),
  previewRemoteImage: vi.fn(async () => ({ src: "jotra-preview://held" })),
}));

vi.mock("../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => mocks.bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
  previewRemoteImage: mocks.previewRemoteImage,
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async () => () => {}),
  emitFrontendReady: vi.fn(async () => {}),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(
      (id: string) => mocks.heldReads.get(id) ?? Promise.resolve(mocks.bufferContent.get(id) ?? ""),
    ),
  },
}));

const WEB = "https://example.com/a.png";

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

function HoldLayout(props: { ids: string[] }) {
  const win = useWindow();
  for (const id of props.ids) win.layout.setLocal(id, { kind: "inline" });
  return null;
}

describe("EditorInstance: web images across a tab switch", () => {
  beforeAll(async () => {
    await loadGrammar("markdown");
  });

  afterEach(() => {
    cleanup();
    mocks.heldReads.clear();
    mocks.bufferContent.clear();
    mocks.previewRemoteImage.mockClear();
  });

  it("loads a web image for the tab the view still shows while the next one opens", async () => {
    mocks.bufferContent.set("R1", `![](${WEB})\n`);
    mocks.heldReads.set("R2", new Promise<string>(() => {}));
    const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
    const [buffer, setBuffer] = createSignal(mockBuffer("R1", "first.md"));
    const { container } = render(() => (
      <WindowProvider windowId={9701}>
        <HoldLayout ids={["R1", "R2"]} />
        <EditorInstance buffer={buffer()} />
      </WindowProvider>
    ));
    const button = await vi.waitFor(() => {
      const found = container.querySelector<HTMLButtonElement>(".cm-md-image-download");
      expect(found?.textContent).toBe("Load image");
      return found!;
    });

    setBuffer(mockBuffer("R2", "second.md"));
    await Promise.resolve();
    expect(container.querySelector(".cm-md-image-download")).toBe(button);
    button.click();

    await vi.waitFor(() => expect(mocks.previewRemoteImage).toHaveBeenCalled());
    expect(mocks.previewRemoteImage).toHaveBeenCalledWith("R1", WEB, "asked");
  });
});
