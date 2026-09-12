import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
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
  chatCancel: vi.fn(),
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
  return {
    default: (props: { buffer: { id: string } }) => {
      const win = useWindow();
      createEffect(() => win.editor.setCurrentBufferId(props.buffer.id));
      onCleanup(() => win.editor.setCurrentBufferId(null));
      return <div data-testid="editor-stub" />;
    },
  };
});

vi.spyOn(configStore, "config").mockImplementation(() => mocks.config());
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
      font_family: "monospace",
      font_size: 14,
      word_wrap: true,
      tab_size: 2,
      autosave_debounce_ms: 300,
      markdown_typography: true,
      markdown_editing: true,
      status_bar: false,
    },
    window: { width: 1100, height: 720, maximized: false },
    keybindings: {},
    history: { max_entries: 500 },
    storage: { path: "~/.writ" },
    theme: { preset: "warp-dark", overrides: {} },
    appearance: {
      polarity: "system",
      accent: "pine",
      prose_face: "system",
      interface_text_size: null,
    },
    commands: { usage: {} },
    workspace: { root: null },
    inbox: { path: null, focus: true },
    updater: { auto_check: true },
    ai: {
      enabled: false,
      preset: "ollama",
      base_url: "http://localhost:11434/v1",
      model: "",
      consented_hosts: [],
      chat: {
        enabled: true,
        provider: "openai_compatible",
        base_url: "http://localhost:11434/v1",
        model: "llama3",
      },
    },
    mcp: { enabled: false, approved_clients: [] },
    spelling: { enabled: false, dialect: "american", ignored_words: [] },
    preview: {
      default_layout_html: "split",
      default_layout_markdown: "split",
      live_render_threshold_mb: 1,
      render_confirm_threshold_mb: 5,
      render_refuse_threshold_mb: 50,
      debounce_ms: 200,
      run_scripts: true,
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
