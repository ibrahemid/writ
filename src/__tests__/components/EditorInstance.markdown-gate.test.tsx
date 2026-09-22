import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import type { LayoutMode } from "../../lib/preview-layout";
import { getCommand } from "../../commands/registry";
import { WIKILINK_CLASS } from "../../editor/wikilink-decorations";

// The Markdown extensions load for a Markdown file, keyed on the file's
// extension rather than on the detected language: a scratch buffer with no
// extension detects as markdown but is not a Markdown file. The decorations
// additionally need the inline layout, which PreviewLayout resolves in the
// app and this harness sets directly.

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

// Renders before the editor, so the layout is in place by the time the view
// is built. PreviewLayout does this in the app, off an async read.
function HoldLayout(props: { bufferId: string; layout: LayoutMode }) {
  useWindow().layout.setLocal(props.bufferId, props.layout);
  return null;
}

async function mount(buffer: BufferDocument, layout: LayoutMode = { kind: "inline" }) {
  const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
  const [buf] = createSignal(buffer);
  const result = render(() => (
    <WindowProvider windowId={9501}>
      <HoldLayout bufferId={buffer.id} layout={layout} />
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

  it("loads no markdown decorations for a markdown buffer laid out as source", async () => {
    bufferContent.set("G6", MD_SOURCE);
    const { container } = await mount(mockBuffer("G6", "plan.md", "/files/plan.md"), {
      kind: "source",
    });

    expect(container.querySelector(".cm-line-md-h1")).toBeNull();
    // The wikilink layer is the file type's, not the layout's.
    expect(container.querySelector(`.${WIKILINK_CLASS}`)).not.toBeNull();
  });

  it("puts the decorations back when the layout returns to inline", async () => {
    bufferContent.set("G7", MD_SOURCE);
    const buffer = mockBuffer("G7", "plan.md", "/files/plan.md");
    const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
    const [buf] = createSignal(buffer);
    let setLayout: ((layout: LayoutMode) => void) | null = null;
    function Harness() {
      const win = useWindow();
      win.layout.setLocal(buffer.id, { kind: "source" });
      setLayout = (layout) => win.layout.setLocal(buffer.id, layout);
      return null;
    }
    const { container } = render(() => (
      <WindowProvider windowId={9501}>
        <Harness />
        <EditorInstance buffer={buf()} />
      </WindowProvider>
    ));
    await flushMicrotasks();
    expect(container.querySelector(".cm-line-md-h1")).toBeNull();
    setLayout!({ kind: "inline" });
    await flushMicrotasks();
    expect(container.querySelector(".cm-line-md-h1")).not.toBeNull();
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
