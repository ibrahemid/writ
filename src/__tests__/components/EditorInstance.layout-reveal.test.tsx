import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import type { Transaction } from "@codemirror/state";
import type { BufferDocument } from "../../types/buffer";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import type { LayoutMode } from "../../lib/preview-layout";

// Inline and Source lay a Markdown file out at different line heights, and
// CodeMirror keeps the top line where it was, so a caret near the bottom of
// the pane ends up below it. The switch asks for the caret line in view.

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

const MD_SOURCE = "# Sourdough\n\n- [ ] Feed the starter\n- [ ] Buy rye\n";

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

async function mount(buffer: BufferDocument, layouts: Record<string, LayoutMode>) {
  const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
  const [buf, setBuf] = createSignal(buffer);
  let setLayout!: (id: string, layout: LayoutMode) => void;
  function Layouts() {
    const win = useWindow();
    for (const [id, layout] of Object.entries(layouts)) win.layout.setLocal(id, layout);
    setLayout = (id, layout) => win.layout.setLocal(id, layout);
    return null;
  }
  const result = render(() => (
    <WindowProvider windowId={9503}>
      <Layouts />
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return { ...result, setBuf, setLayout };
}

function viewIn(container: HTMLElement): EditorView {
  const dom = container.querySelector<HTMLElement>(".cm-editor");
  const view = dom ? EditorView.findFromDOM(dom) : null;
  if (!view) throw new Error("no editor view mounted");
  return view;
}

/** Every transaction the view applies from here on. */
function recordTransactions(view: EditorView): Transaction[] {
  const seen: Transaction[] = [];
  const update = view.update.bind(view);
  vi.spyOn(view, "update").mockImplementation((transactions) => {
    seen.push(...transactions);
    update(transactions);
  });
  return seen;
}

function caretAtEnd(view: EditorView): number {
  const end = view.state.doc.length;
  view.dispatch({ selection: { anchor: end } });
  return end;
}

describe("EditorInstance: switching a Markdown file's layout keeps the caret line in view", () => {
  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("asks for the caret line in view on a switch from Inline to Source", async () => {
    bufferContent.set("L1", MD_SOURCE);
    const { container, setLayout } = await mount(mockBuffer("L1", "bread.md", "/files/bread.md"), {
      L1: { kind: "inline" },
    });
    const view = viewIn(container);
    const caret = caretAtEnd(view);
    expect(container.querySelector(".cm-line-md-h1")).not.toBeNull();
    const seen = recordTransactions(view);

    setLayout("L1", { kind: "source" });

    expect(container.querySelector(".cm-line-md-h1")).toBeNull();
    expect(seen.some((tr) => tr.scrollIntoView)).toBe(true);
    expect(view.state.selection.main.head).toBe(caret);
  });

  it("asks for the caret line in view on a switch from Source to Inline", async () => {
    bufferContent.set("L2", MD_SOURCE);
    const { container, setLayout } = await mount(mockBuffer("L2", "bread.md", "/files/bread.md"), {
      L2: { kind: "source" },
    });
    const view = viewIn(container);
    const caret = caretAtEnd(view);
    expect(container.querySelector(".cm-line-md-h1")).toBeNull();
    const seen = recordTransactions(view);

    setLayout("L2", { kind: "inline" });

    expect(container.querySelector(".cm-line-md-h1")).not.toBeNull();
    expect(seen.some((tr) => tr.scrollIntoView)).toBe(true);
    expect(view.state.selection.main.head).toBe(caret);
  });

  it("leaves the outgoing view alone when a tab with another layout is brought forward", async () => {
    bufferContent.set("L3", MD_SOURCE);
    bufferContent.set("L4", MD_SOURCE);
    const { container, setBuf } = await mount(mockBuffer("L3", "bread.md", "/files/bread.md"), {
      L3: { kind: "inline" },
      L4: { kind: "source" },
    });
    const outgoing = viewIn(container);
    const seen = recordTransactions(outgoing);

    setBuf(mockBuffer("L4", "rye.md", "/files/rye.md"));

    expect(container.querySelector(".cm-line-md-h1")).not.toBeNull();
    expect(seen.some((tr) => tr.scrollIntoView)).toBe(false);
    await flushMicrotasks();
    expect(viewIn(container)).not.toBe(outgoing);
    expect(container.querySelector(".cm-line-md-h1")).toBeNull();
  });
});
