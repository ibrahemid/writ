import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import type { Accessor } from "solid-js";
import { configStore } from "../../stores/global/config";
import type { WritConfig } from "../../types/config";
import type { BufferDocument } from "../../types/buffer";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn().mockResolvedValue(null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  restoreNoteFile: vi.fn(),
  materialiseNote: vi.fn().mockResolvedValue(undefined),
  cancelMaterialiseNote: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async () => () => undefined),
}));

const mocks = vi.hoisted(() => ({
  config: vi.fn(),
  activeBuffer: (() => null) as Accessor<BufferDocument | null>,
  win: { editor: {}, tabs: {} } as unknown as Record<string, unknown>,
}));

vi.mock("../../components/Preview/PreviewLayout", () => ({
  default: () => <div data-testid="preview-layout" />,
}));
vi.mock("../../components/Toolbar/Toolbar", () => ({ default: () => null }));
vi.mock("../../components/Editor/TabBar", () => ({ default: () => null }));
vi.mock("../../components/Find/FindOverlay", () => ({ default: () => null }));
vi.mock("../../components/Editor/SpellingPreview", () => ({ default: () => null }));
vi.mock("../../components/Editor/StatusBar", () => ({ default: () => null }));
vi.mock("../../lib/use-active-buffer", () => ({ useActiveBuffer: () => mocks.activeBuffer }));
vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => mocks.win,
}));

vi.spyOn(configStore, "config").mockImplementation(mocks.config);

import EditorArea from "../../components/Editor/EditorArea";
import { createEditorStore } from "../../stores/window/editor-store";
import { createDownloadStore } from "../../stores/window/download-store";
import { collectUnsavedContent, resetAutosave } from "../../services/autosave";

const PATH = "/notes/away.md";

function configWithoutStatusBar(): WritConfig {
  return {
    hotkey: { toggle: "" },
    sidebar: { toggle: "", default_visible: false, position: "left", open: true, width: 240 },
    panel: { open: false, width: 240 },
    editor: {
      font_family: "monospace",
      font_size: 16,
      word_wrap: true,
      tab_size: 2,
      autosave_debounce_ms: 300,
      markdown_typography: true,
      markdown_editing: true,
      status_bar: false,
    },
    window: { width: 800, height: 600, maximized: false },
    keybindings: {},
    history: { max_entries: 500 },
    storage: { path: "~/.writ" },
    theme: { preset: "writ-light", overrides: {} },
    appearance: { polarity: "system", accent: "pine", prose_face: "system" },
    commands: { usage: {} },
    preview: {
      default_layout_html: "split",
      default_layout_markdown: "split",
      live_render_threshold_mb: 1,
      render_confirm_threshold_mb: 5,
      render_refuse_threshold_mb: 50,
      debounce_ms: 200,
      run_scripts: true,
    },
    workspace: { root: null },
    inbox: { path: null, focus: true },
    updater: { auto_check: true },
    ai: {
      enabled: false,
      preset: "ollama",
      base_url: "http://localhost:11434/v1",
      model: "",
      consented_hosts: [],
    },
    spelling: { enabled: false, dialect: "american", ignored_words: [] },
  };
}

/** The window as it stands while a note waits on its bytes: no buffer behind it. */
async function windowWithADownloadRunning() {
  const editor = createEditorStore();
  const downloads = createDownloadStore();
  mocks.win = {
    editor,
    tabs: { closeTab: vi.fn() },
    downloads,
  } as unknown as Record<string, unknown>;
  await downloads.start({ path: PATH, title: "away.md", provider: "iCloud Drive" });
  return { editor, downloads };
}

beforeEach(() => {
  mocks.config.mockReturnValue(configWithoutStatusBar());
  mocks.activeBuffer = () => null;
});

afterEach(() => {
  cleanup();
  resetAutosave();
  vi.clearAllMocks();
});

describe("a tab whose note is still downloading", () => {
  it("is not a note the editor store holds anything about", async () => {
    // A download has a path and a tab; it has no buffer, so nothing ever
    // hands its path to `noteOpened`. Every predicate the change question
    // rests on answers about a note the store was told to measure, and this
    // path is not one.
    const { editor, downloads } = await windowWithADownloadRunning();

    expect(downloads.pending()).toHaveLength(1);
    expect(downloads.pending()[0]?.state).toBe("downloading");

    expect(editor.savesAreHeld(PATH)).toBe(false);
    expect(editor.isTracked(PATH)).toBe(false);
    expect(editor.noteFileState(PATH)).toBe("present");
    expect(editor.isRemovedOnDisk(PATH)).toBe(false);
    expect(editor.isFileChangedOnDisk(PATH)).toBe(false);
    expect(collectUnsavedContent()).toEqual([]);

    // `isDirty` fails closed for a note it holds no record of, and a download
    // path is the widest such case. It stays that way: no caller reaches it
    // with one, and the mark asks `isTracked` first.
    expect(editor.isDirty(PATH)).toBe(true);

    editor.stopSaveListener();
  });

  it("puts no bar over the pane it is waiting in", async () => {
    // The download branch of `openFile` clears the active tab, so the editor
    // holds no note and the state the bars are switched on reads present.
    const { editor } = await windowWithADownloadRunning();

    const { queryByRole } = render(() => <EditorArea />);

    expect(queryByRole("alertdialog")).toBeNull();
    expect(queryByRole("alert")).toBeNull();

    editor.stopSaveListener();
  });

  it("does not stop the bar going up for a note that is open", async () => {
    // Anchors the case above: the same render puts the question on screen for
    // a note the editor is holding, so the absence there is the download and
    // not a harness that renders no bar at all.
    const { editor } = await windowWithADownloadRunning();
    mocks.activeBuffer = () => ({ id: "b1" }) as unknown as BufferDocument;
    editor.setCurrentBufferId("b1");
    editor.recordFileEvent("b1", "modified");

    const { queryByRole } = render(() => <EditorArea />);

    expect(queryByRole("alertdialog")).not.toBeNull();

    editor.stopSaveListener();
  });
});
