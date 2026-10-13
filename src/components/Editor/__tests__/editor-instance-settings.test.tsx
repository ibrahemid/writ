import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import { indentUnit } from "@codemirror/language";
import type { BufferDocument } from "../../../types/buffer";
import type { WritConfig } from "../../../types/config";
import WindowProvider from "../../WindowProvider/WindowProvider";

// Settings, Editor: Word wrap and Tab size reach the open editor, at mount and
// when they change while a file is open.

const bufferContent = new Map<string, string>();

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
  updateConfig: vi.fn(async () => {}),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

import { configStore } from "../../../stores/global/config";

function mockBuffer(id: string, filename: string, sizeBytes = 0): BufferDocument {
  return {
    id,
    title: filename,
    filename,
    status: "active",
    language: null,
    source_path: `/files/${filename}`,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: sizeBytes,
    line_ending: "lf",
  };
}

// The class soft wrapping puts on the content, which is what the CSS wraps on.
// `view.lineWrapping` follows it only after a layout measure, which jsdom
// never runs.
function wraps(view: EditorView): boolean {
  return view.contentDOM.classList.contains("cm-lineWrapping");
}

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function setEditor(change: Partial<WritConfig["editor"]>): Promise<void> {
  const current = configStore.config();
  await configStore.save({ ...current, editor: { ...current.editor, ...change } });
}

async function mount(buffer: BufferDocument): Promise<EditorView> {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buf] = createSignal(buffer);
  const { container } = render(() => (
    <WindowProvider windowId={9801}>
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  const dom = container.querySelector<HTMLElement>(".cm-editor");
  if (!dom) throw new Error("the editor did not mount");
  const view = EditorView.findFromDOM(dom);
  if (!view) throw new Error("no view behind the editor");
  return view;
}

describe("EditorInstance: Word wrap", () => {
  beforeEach(async () => {
    bufferContent.clear();
    await setEditor({ word_wrap: true, tab_size: 2 });
  });

  afterEach(() => {
    cleanup();
  });

  it("wraps lines when Word wrap is on", async () => {
    bufferContent.set("W1", "a long line of prose\n");
    const view = await mount(mockBuffer("W1", "notes.txt"));

    expect(wraps(view)).toBe(true);
  });

  it("leaves lines unwrapped when Word wrap is off", async () => {
    await setEditor({ word_wrap: false });
    bufferContent.set("W2", "a long line of prose\n");
    const view = await mount(mockBuffer("W2", "notes.txt"));

    expect(wraps(view)).toBe(false);
  });

  it("follows the setting while the file is open, both ways", async () => {
    bufferContent.set("W3", "a long line of prose\n");
    const view = await mount(mockBuffer("W3", "notes.txt"));
    expect(wraps(view)).toBe(true);

    await setEditor({ word_wrap: false });
    expect(wraps(view)).toBe(false);

    await setEditor({ word_wrap: true });
    expect(wraps(view)).toBe(true);
  });

  it("never wraps a large file, whatever the setting says", async () => {
    bufferContent.set("W4", "alpha\nbeta\n");
    const view = await mount(mockBuffer("W4", "huge.log", 6 * 1024 * 1024));
    expect(wraps(view)).toBe(false);

    await setEditor({ word_wrap: false });
    await setEditor({ word_wrap: true });
    expect(wraps(view)).toBe(false);
  });
});

describe("EditorInstance: Tab size", () => {
  beforeEach(async () => {
    bufferContent.clear();
    await setEditor({ word_wrap: true, tab_size: 2 });
  });

  afterEach(() => {
    cleanup();
  });

  it("draws a tab as wide as Tab size and indents by that many spaces", async () => {
    await setEditor({ tab_size: 8 });
    bufferContent.set("T1", "\tindented\n");
    const view = await mount(mockBuffer("T1", "script.py"));

    expect(view.state.tabSize).toBe(8);
    expect(view.state.facet(indentUnit)).toBe(" ".repeat(8));
  });

  it("follows the setting while the file is open", async () => {
    bufferContent.set("T2", "\tindented\n");
    const view = await mount(mockBuffer("T2", "script.py"));
    expect(view.state.tabSize).toBe(2);
    expect(view.state.facet(indentUnit)).toBe("  ");

    await setEditor({ tab_size: 4 });

    expect(view.state.tabSize).toBe(4);
    expect(view.state.facet(indentUnit)).toBe("    ");
  });

  it("applies to a large file too", async () => {
    await setEditor({ tab_size: 8 });
    bufferContent.set("T3", "\tcol\n");
    const view = await mount(mockBuffer("T3", "huge.tsv", 6 * 1024 * 1024));

    expect(view.state.tabSize).toBe(8);
  });
});
