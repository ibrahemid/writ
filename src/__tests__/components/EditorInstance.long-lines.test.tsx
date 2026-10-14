import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { EditorView } from "@codemirror/view";
import type { BufferDocument } from "../../types/buffer";
import { loadGrammar } from "../../editor/languages";
import { MAX_NORMAL_LINE_LENGTH, WRAP_LINE_LIMIT } from "../../editor/large-file";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import type { WindowState } from "../../stores/window/createWindowState";

// A Markdown note holding one pasted data URI used to lose all of its
// rendering, wrapping and highlighting for that one line (EP-09). The line is
// now drawn plain and the rest of the note keeps them.

const bufferContent = new Map<string, string>();

vi.mock("../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  setBufferViewPosition: vi.fn(async () => {}),
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

async function flushMicrotasks(n = 30) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(buffer: BufferDocument) {
  const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
  let win!: WindowState;
  function Capture() {
    win = useWindow();
    win.layout.setLocal(buffer.id, { kind: "inline" });
    return null;
  }
  const result = render(() => (
    <WindowProvider windowId={9640}>
      <Capture />
      <EditorInstance buffer={buffer} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  const dom = result.container.querySelector<HTMLElement>(".cm-editor");
  const view = dom ? EditorView.findFromDOM(dom) : null;
  if (!view) throw new Error("no editor view mounted");
  return { ...result, view, win };
}

describe("EditorInstance: a note with one very long line", () => {
  beforeAll(async () => {
    await loadGrammar("markdown");
  });

  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("opens as LongLines and keeps rendering, wrapping and highlighting the rest", async () => {
    const blob = `![photo](data:image/png;base64,${"A".repeat(MAX_NORMAL_LINE_LENGTH * 3)})`;
    bufferContent.set("M", `# Trip notes\n\n${blob}\n\nSome **bold** words.\n`);
    const { container, view, win } = await mount(mockBuffer("M", "trip.md"));

    expect(win.editor.largeFileMode()).toEqual({ kind: "LongLines" });
    expect(container.querySelector(".cm-line-md-h1")).not.toBeNull();
    expect(view.contentDOM.classList.contains("cm-lineWrapping")).toBe(true);
    expect(win.editor.language()).toBe("markdown");
  });

  it("starts with wrapping off for a line past the wrap limit, and wraps once asked", async () => {
    bufferContent.set("J", `{"data":"${"z".repeat(WRAP_LINE_LIMIT + 10)}"}`);
    const { view, win } = await mount(mockBuffer("J", "dump.json"));

    expect(view.contentDOM.classList.contains("cm-lineWrapping")).toBe(false);
    win.editor.setOverride("J", "wrap", true);
    await flushMicrotasks();
    expect(view.contentDOM.classList.contains("cm-lineWrapping")).toBe(true);
  });
});
