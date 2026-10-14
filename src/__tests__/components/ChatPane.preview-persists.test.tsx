import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import { createRoot, createSignal } from "solid-js";
import { configStore } from "../../stores/global/config";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import type { BufferDocument } from "../../types/buffer";
import type { WritConfig } from "../../types/config";

// The chat column is a sibling of the note, not a layer over it, and opening
// it must leave the preview's iframe element where it is: taking a loaded
// writ-preview:// iframe out of the page freezes the macOS webview (PR #127).
// The column can take the room, the iframe keeps its element.

const mocks = vi.hoisted(() => ({
  forceRender: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  config: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: mocks.forceRender,
  previewRender: mocks.forceRender,
  previewClose: mocks.previewClose,
  previewGetLayout: mocks.previewGetLayout,
  previewSetLayout: mocks.previewSetLayout,
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
  chatSend: vi.fn(),
  chatStop: vi.fn(),
  chatState: vi.fn(),
  chatApplyProposal: vi.fn(),
  chatDiscardProposal: vi.fn(),
}));

vi.mock("../../commands/chat", () => ({
  byteLabel: (bytes: number) => `${bytes} bytes`,
  sendChatMessage: vi.fn(),
}));

vi.mock("../../components/Editor/EditorInstance", async () => {
  const { createEffect, onCleanup } = await import("solid-js");
  const { useWindow } = await import("../../components/WindowProvider/WindowProvider");
  const { showInEditor } = await import("../fixtures/editor-view");
  return {
    default: (props: { buffer: { id: string } }) => {
      const win = useWindow();
      createEffect(() => showInEditor(win, props.buffer.id, ""));
      onCleanup(() => {
        win.editor.registerView(null);
        win.editor.setCurrentBufferId(null);
      });
      return <div data-testid="editor-stub" />;
    },
  };
});

// The column's open state is the config's (F17), so the fixture's copy of it
// follows the column's own setter.
const [chatColumnOpen, setChatColumnOpen] = createRoot(() => createSignal(false));
vi.spyOn(configStore, "loaded").mockImplementation(() => true);
vi.spyOn(configStore, "setChatPanelOpen").mockImplementation((open) => setChatColumnOpen(open));
vi.spyOn(configStore, "config").mockImplementation(() => {
  const config = mocks.config() as WritConfig;
  return { ...config, chat_panel: { ...config.chat_panel, open: chatColumnOpen() } };
});
vi.spyOn(bufferRegistry, "activeTabs").mockImplementation(() => []);

import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { windowRegistry } from "../../stores/global/window-registry";
import PreviewLayout from "../../components/Preview/PreviewLayout";
import ChatPane from "../../components/Chat/ChatPane";

function htmlBuffer(): BufferDocument {
  return {
    id: "H1",
    title: "page.html",
    filename: "page.html",
    status: "active",
    language: null,
    source_path: null,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: "",
    updated_at: "",
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
    encoding: "UTF-8",
  };
}

function chatConfig(): WritConfig {
  return {
    hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
    sidebar: {
      toggle: "CmdOrCtrl+\\",
      default_visible: false,
      position: "left",
      open: false,
      width: 240,
      collapsed: [],
      hidden: [],
    },
    panel: { open: false, width: 240 },
    chat_panel: { open: false, width: 380 },
    first_run: { hint_dismissed: false },
    editor: {
      font_size: 14,
      word_wrap: true,
      tab_size: 2,
      autosave_debounce_ms: 300,
      markdown_editing: true,
      status_bar: false,
      status_bar_counts: false,
    },
    window: { width: 1100, height: 720, maximized: false },
    keybindings: {},
    history: { max_entries: 500 },
    theme: { preset: "warp-dark", overrides: {} },
    appearance: {
      polarity: "system",
      accent: "pine",
      prose_face: "system",
      interface_text_size: null,
    },
    files: { default_extension: "txt" },
    workspace: { root: null },
    inbox: { path: null, focus: true },
    updater: { auto_check: true },
    ai: {
      provider: "ollama",
      base_url: "",
      model: "",
      consented_hosts: [],
      rewrite: { enabled: false },
      chat: { enabled: true, model: "llama3", model_provider: "" },
    },
    mcp: { enabled: false, approved_clients: [] },
    apps: { connections: false, graph: false, tags: false },
    spelling: { enabled: false, dialect: "american", ignored_words: [] },
    preview: {
      default_layout_html: "split",
      default_layout_markdown: "inline",
      live_render_threshold_mb: 1,
      render_confirm_threshold_mb: 5,
      render_refuse_threshold_mb: 50,
      debounce_ms: 200,
    },
  };
}

function frame(container: HTMLElement): HTMLIFrameElement | null {
  return container.querySelector<HTMLIFrameElement>("iframe.preview-frame");
}

describe("the preview while the chat column is open", () => {
  beforeEach(() => {
    mocks.forceRender.mockClear();
    mocks.config.mockReturnValue(chatConfig());
    rendererRegistry.setFromIpc([
      {
        content_type: "html",
        capabilities: {
          supports_live_render: true,
          supports_print: true,
          max_safe_document_bytes: 50 * 1024 * 1024,
        },
      },
    ]);
  });

  afterEach(() => {
    cleanup();
    rendererRegistry.setFromIpc([]);
  });

  it("is the same element it was, with the column open and again once it closes", async () => {
    const { container } = render(() => (
      <WindowProvider windowId={9301}>
        <PreviewLayout buffer={htmlBuffer()} />
        <ChatPane />
      </WindowProvider>
    ));

    await waitFor(() => expect(frame(container)!.src).toMatch(/document\/H1\?v=[1-9]\d*$/));
    const original = frame(container);
    expect(original).not.toBeNull();

    const win = windowRegistry.getActive();
    win!.chatPanel.show();

    await waitFor(() =>
      expect(container.querySelector(".chat-pane")?.classList.contains("is-open")).toBe(true),
    );
    expect(frame(container)).toBe(original);
    expect(frame(container)!.src).toMatch(/document\/H1\?v=[1-9]\d*$/);
    expect(container.querySelector(".preview-pane-slot")).not.toBeNull();

    win!.chatPanel.hide();
    await waitFor(() =>
      expect(container.querySelector(".chat-pane")?.classList.contains("is-open")).toBe(false),
    );
    expect(frame(container)).toBe(original);
  });
});
