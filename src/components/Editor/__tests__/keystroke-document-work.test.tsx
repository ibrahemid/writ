import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { BufferDocument } from "../../../types/buffer";
import type { WritConfig } from "../../../types/config";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import WordCount from "../WordCount";
import TokenEstimate from "../TokenEstimate";

const h = vi.hoisted(() => ({
  content: new Map<string, string>(),
  estimate: vi.fn(async (_text: string) => 1),
  spelling: vi.fn(async (_text: string) => [] as unknown[]),
}));

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => h.content.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => "d".repeat(64)),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  promptEstimateTokens: h.estimate,
  checkSpelling: h.spelling,
  updateConfig: vi.fn(async () => {}),
  recordUnsavedNotes: vi.fn(async () => {}),
  discardUnsavedText: vi.fn(async () => {}),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => h.content.get(id) ?? ""),
  },
}));

import { configStore } from "../../../stores/global/config";
import { resetAutosave } from "../../../services/autosave";

const LINES = 4000;

function prose(lines: number): string {
  const out: string[] = [];
  for (let i = 0; i < lines; i++) out.push(`Line ${i} has a few plain words in it, and a comma.`);
  return out.join("\n");
}

function note(id: string): BufferDocument {
  return {
    id,
    title: "notes.txt",
    filename: "notes.txt",
    status: "active",
    language: null,
    source_path: "/files/notes.txt",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 200_000,
    line_ending: "lf",
    encoding: "UTF-8",
  };
}

async function settle(ms: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, ms));
}

async function configure(): Promise<void> {
  const current = configStore.config();
  const next: WritConfig = {
    ...current,
    editor: { ...current.editor, status_bar_counts: true, autosave_debounce_ms: 300 },
    spelling: { ...current.spelling, enabled: true },
  };
  await configStore.save(next);
}

type Win = ReturnType<typeof useWindow>;

async function mount(): Promise<{ view: EditorView; win: Win }> {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buffer] = createSignal(note("K1"));
  let win: Win | null = null;
  function Probe() {
    win = useWindow();
    return null;
  }
  const { container } = render(() => (
    <WindowProvider windowId={9811}>
      <Probe />
      <EditorInstance buffer={buffer()} />
      <WordCount class="counts" characters />
      <TokenEstimate />
    </WindowProvider>
  ));
  for (let i = 0; i < 20; i++) await Promise.resolve();
  const dom = container.querySelector<HTMLElement>(".cm-editor");
  if (!dom) throw new Error("the editor did not mount");
  const view = EditorView.findFromDOM(dom);
  if (!view) throw new Error("no view behind the editor");
  if (!win) throw new Error("no window store");
  return { view, win };
}

describe("a keystroke in a Normal-tier note", () => {
  beforeEach(async () => {
    h.content.clear();
    h.estimate.mockClear();
    h.spelling.mockClear();
    await configure();
  });

  afterEach(() => {
    cleanup();
    resetAutosave();
    vi.restoreAllMocks();
  });

  it("materialises nothing, sends no whole text and hashes nothing", async () => {
    const text = prose(LINES);
    h.content.set("K1", text);
    const { view } = await mount();
    await settle(900);
    h.estimate.mockClear();
    h.spelling.mockClear();

    const toString = vi.spyOn(Text.prototype, "toString");
    const digest = vi.spyOn(crypto.subtle, "digest");

    view.dispatch({ selection: { anchor: view.state.doc.length } });
    for (const char of "typed words") {
      view.dispatch(view.state.replaceSelection(char), { userEvent: "input.type" });
    }
    const typingCalls = toString.mock.calls.length;
    const length = view.state.doc.length;

    await settle(1200);

    expect.soft(typingCalls, "a keystroke materialised the document").toBe(0);
    const estimated = h.estimate.mock.calls.map(([sent]) => sent.length);
    expect.soft(estimated.filter((sent) => sent >= length / 2), "the token estimate sent the whole text").toEqual([]);
    const linted = h.spelling.mock.calls.map(([sent]) => sent.length);
    expect.soft(linted.length, "the edit was not linted again").toBeGreaterThan(0);
    expect.soft(linted.filter((sent) => sent >= length / 2), "spelling sent the whole text").toEqual([]);
    expect.soft(digest, "the document was hashed after the pause").not.toHaveBeenCalled();
  });

  it("materialises nothing while the note's file changed under it and its saves are held", async () => {
    const text = prose(LINES);
    h.content.set("K1", text);
    const { view, win } = await mount();
    await settle(900);
    win.editor.recordFileEvent("K1", "modified");
    expect(win.editor.savesAreHeld("K1")).toBe(true);

    const toString = vi.spyOn(Text.prototype, "toString");
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    for (const char of "typed words") {
      view.dispatch(view.state.replaceSelection(char), { userEvent: "input.type" });
    }
    const typingCalls = toString.mock.calls.length;
    toString.mockRestore();

    expect(typingCalls, "a keystroke under the bar materialised the document").toBe(0);
    expect(win.editor.textOfRemoved("K1")).toBe(`${text}typed words`);
  });
});
