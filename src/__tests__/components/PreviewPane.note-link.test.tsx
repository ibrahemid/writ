import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor } from "@solidjs/testing-library";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import type { BufferDocument } from "../../types/buffer";

const NOTES_ROOT = "/notes";
const FROM = "/notes/From.md";

const mocks = vi.hoisted(() => ({
  forceRender: vi.fn().mockResolvedValue({
    kind: "rendered" as const,
    used_fallback_stylesheet: true,
    parser_warnings: [],
  }),
  classify: vi.fn(),
  openExternalUrl: vi.fn().mockResolvedValue(undefined),
  getNotesRoot: vi.fn().mockResolvedValue("/notes"),
  resolveNoteLink: vi.fn(),
  noteHeadingLine: vi.fn(),
  openFile: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  previewListRenderers: vi.fn().mockResolvedValue([]),
  previewForceRender: mocks.forceRender,
  previewRender: mocks.forceRender,
  previewClose: vi.fn().mockResolvedValue(undefined),
  previewGetLayout: vi.fn().mockResolvedValue(null),
  previewSetLayout: vi.fn().mockResolvedValue(undefined),
  searchBuffers: vi.fn().mockResolvedValue([]),
  getConfig: vi.fn(),
  updateConfig: vi.fn().mockResolvedValue(undefined),
  classifyExternalUrl: mocks.classify,
  openExternalUrl: mocks.openExternalUrl,
  getNotesRoot: mocks.getNotesRoot,
  resolveNoteLink: mocks.resolveNoteLink,
  noteHeadingLine: mocks.noteHeadingLine,
  noteNameCandidates: vi.fn().mockResolvedValue([]),
  newNamedNote: vi.fn(),
  openFile: mocks.openFile,
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../../components/Editor/EditorInstance", async () => {
  const { createEffect, onCleanup } = await import("solid-js");
  const { useWindow } = await import("../../components/WindowProvider/WindowProvider");
  return {
    default: (props: { buffer: { id: string } }) => {
      const win = useWindow();
      createEffect(() => win.editor.setCurrentBufferId(props.buffer.id));
      onCleanup(() => win.editor.setCurrentBufferId(null));
      // The reveal a real editor consumes, exposed so a test can read what the
      // preview asked for.
      const reveal = () => {
        const held = win.editor.pendingReveal();
        return held ? `${held.bufferId}@${held.line}` : "";
      };
      return <div data-testid="editor-stub" data-reveal={reveal()} />;
    },
  };
});

import PreviewLayout from "../../components/Preview/PreviewLayout";
import { linkStore } from "../../stores/global/link";
import { notesStore } from "../../stores/global/notes";

function noteBuffer(): BufferDocument {
  return {
    id: "N1",
    title: "From.md",
    filename: "From.md",
    status: "active",
    language: null,
    source_path: FROM,
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

function opened(path: string) {
  return {
    doc: { ...noteBuffer(), id: `open:${path}`, source_path: path, filename: path },
    mode: { kind: "Normal" as const },
  };
}

async function mountPreview() {
  const view = render(() => (
    <WindowProvider windowId={7402}>
      <PreviewLayout buffer={noteBuffer()} />
    </WindowProvider>
  ));
  const frame = await waitFor(() => {
    const el = view.container.querySelector<HTMLIFrameElement>("iframe.preview-frame");
    expect(el).not.toBeNull();
    return el!;
  });
  return { ...view, frame };
}

function sendLinkOpen(frame: HTMLIFrameElement, href: string) {
  window.dispatchEvent(
    new MessageEvent("message", {
      data: { source: "writ-preview", dir: "up", type: "link:open", href, x: 10, y: 10 },
      source: frame.contentWindow as MessageEventSource | null,
    }),
  );
}

// The seam between the renderer's `writ-note:` href and the tab that opens:
// without it a preview wikilink reaches the external-link popover and is
// refused as not being a web address.
describe("a preview link to a note", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    await linkStore.reset();
    mocks.classify.mockImplementation(async (url: string) => ({
      allowed: true,
      url,
      reason: null,
      message: null,
    }));
    mocks.getNotesRoot.mockResolvedValue(NOTES_ROOT);
    mocks.openFile.mockImplementation(async (path: string) => opened(path));
    mocks.resolveNoteLink.mockResolvedValue({
      status: "resolved",
      path: "/notes/Target.md",
      candidates: [],
      heading_line: null,
    });
    mocks.noteHeadingLine.mockResolvedValue(null);
    await notesStore.load();
    rendererRegistry.setFromIpc([
      {
        content_type: "markdown",
        capabilities: {
          supports_live_render: true,
          supports_print: true,
          max_safe_document_bytes: 50 * 1024 * 1024,
        },
      },
    ]);
  });

  afterEach(async () => {
    cleanup();
    rendererRegistry.setFromIpc([]);
    await linkStore.reset();
  });

  it("opens the note the href names and raises no popover", async () => {
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "writ-note:Target.md");

    await waitFor(() => expect(mocks.openFile).toHaveBeenCalledWith("/notes/Target.md"));
    expect(mocks.resolveNoteLink).toHaveBeenCalledWith(FROM, "Target.md");
    expect(container.querySelectorAll(".link-confirm")).toHaveLength(0);
  });

  // The editor lands on the heading a `[[Note#Section]]` names; the preview
  // carries that heading as the href's fragment and lands on the same line.
  // A note no earlier case opened, so the tab is minted here rather than
  // handed back by the tab store.
  it("opens the note at the heading the href names", async () => {
    mocks.resolveNoteLink.mockResolvedValue({
      status: "resolved",
      path: "/notes/Other.md",
      candidates: [],
      heading_line: null,
    });
    mocks.noteHeadingLine.mockResolvedValue(9);
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "writ-note:Other.md#later-part");

    await waitFor(() =>
      expect(
        container.querySelector('[data-testid="editor-stub"]')?.getAttribute("data-reveal"),
      ).toBe("open:/notes/Other.md@9"),
    );
    expect(mocks.openFile).toHaveBeenCalledWith("/notes/Other.md");
    expect(mocks.noteHeadingLine).toHaveBeenCalledWith("/notes/Other.md", "later-part");
    expect(container.querySelectorAll(".link-confirm")).toHaveLength(0);
  });

  it("confirms a note href the index does not know, and opens nothing", async () => {
    mocks.resolveNoteLink.mockResolvedValue({
      status: "missing",
      path: null,
      candidates: [],
      heading_line: null,
    });
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "writ-note:.obsidian/workspace.json");

    await waitFor(() => expect(container.querySelectorAll(".link-confirm")).toHaveLength(1));
    expect(mocks.openFile).not.toHaveBeenCalled();
  });

  it("leaves a web address to the popover", async () => {
    const { container, frame } = await mountPreview();
    sendLinkOpen(frame, "https://example.com/docs");

    await waitFor(() => expect(container.querySelectorAll(".link-confirm")).toHaveLength(1));
    expect(mocks.openFile).not.toHaveBeenCalled();
    expect(mocks.openExternalUrl).not.toHaveBeenCalled();
  });
});
