import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import type { BufferDocument } from "../../../types/buffer";
import type { SpellingConfig, WritConfig } from "../../../types/config";
import type { SpellingLint } from "../../../types/spelling";
import WindowProvider from "../../WindowProvider/WindowProvider";

const NOTE = "teh colour of the sky\n\nanother paragraph";

const h = vi.hoisted(() => ({
  flagged: new Set<string>(),
  sent: [] as string[],
}));

function lintsIn(text: string): SpellingLint[] {
  return [...h.flagged]
    .filter((word) => text.includes(word))
    .map((word) => {
      const at = text.indexOf(word);
      return { fromUtf16: at, toUtf16: at + word.length, kind: "Spelling", message: "", suggestions: [], confident: false };
    });
}

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(async () => NOTE),
  saveBufferContent: vi.fn(async () => "d".repeat(64)),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  checkSpelling: vi.fn(async (text: string) => {
    h.sent.push(text);
    return lintsIn(text);
  }),
  updateConfig: vi.fn(async () => {}),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: { readContent: vi.fn(async () => NOTE) },
}));

import { configStore } from "../../../stores/global/config";
import { spellingEntries } from "../../../editor/spelling";

function note(): BufferDocument {
  return {
    id: "S1",
    title: "sky.md",
    filename: "sky.md",
    status: "active",
    language: null,
    source_path: "/files/sky.md",
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: NOTE.length,
    line_ending: "lf",
  };
}

// Past the spelling store's 400 ms debounce and the check behind it.
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 900));
}

async function saveSpelling(patch: Partial<SpellingConfig>): Promise<void> {
  const current = configStore.config();
  await configStore.save({ ...current, spelling: { ...current.spelling, ...patch } });
}

async function mount(): Promise<EditorView> {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buffer] = createSignal(note());
  const { container } = render(() => (
    <WindowProvider windowId={9931}>
      <EditorInstance buffer={buffer()} />
    </WindowProvider>
  ));
  for (let i = 0; i < 20; i++) await Promise.resolve();
  const dom = container.querySelector<HTMLElement>(".cm-editor");
  if (!dom) throw new Error("the editor did not mount");
  const view = EditorView.findFromDOM(dom);
  if (!view) throw new Error("no view behind the editor");
  return view;
}

function flaggedWords(view: EditorView): string[] {
  return spellingEntries(view.state).map((entry) => entry.word);
}

describe("a change to what the spell checker reads, with a note open", () => {
  beforeEach(() => {
    h.flagged.clear();
    h.sent.length = 0;
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("checks the whole note again when the dialect changes", async () => {
    await saveSpelling({ enabled: true, dialect: "american", ignored_words: [] });
    h.flagged.add("colour");
    const view = await mount();
    await settle();
    expect(h.sent).toEqual([NOTE]);
    expect(flaggedWords(view)).toEqual(["colour"]);

    h.flagged.clear();
    await saveSpelling({ dialect: "british" });
    await settle();

    expect(h.sent).toEqual([NOTE, NOTE]);
    expect(flaggedWords(view)).toEqual([]);
  });

  it("checks the whole note again when the ignore list changes", async () => {
    await saveSpelling({ enabled: true, dialect: "american", ignored_words: ["teh"] });
    const view = await mount();
    await settle();
    expect(h.sent).toEqual([NOTE]);
    expect(flaggedWords(view)).toEqual([]);

    h.flagged.add("teh");
    await saveSpelling({ ignored_words: [] });
    await settle();

    expect(h.sent).toEqual([NOTE, NOTE]);
    expect(flaggedWords(view)).toEqual(["teh"]);
  });

  it("sends nothing when a setting the checker does not read changes", async () => {
    await saveSpelling({ enabled: true, dialect: "american", ignored_words: [] });
    await mount();
    await settle();
    expect(h.sent).toEqual([NOTE]);

    const current = configStore.config();
    const larger: WritConfig = { ...current, editor: { ...current.editor, font_size: current.editor.font_size + 1 } };
    await configStore.save(larger);
    await saveSpelling({ ignored_words: [] });
    await settle();

    expect(h.sent).toEqual([NOTE]);
  });
});
