import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import { getCommand } from "../../commands/registry";

// F17/F05: an untitled note (source_path === null) must keep its markdown
// surface no matter what its content looks like, except when it opens with a
// shebang, where that shebang's own detection stands. A file opened from
// disk (source_path set) keeps today's filename+content detection untouched.

const bufferContent = new Map<string, string>();

vi.mock("../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

function mockBuffer(
  id: string,
  filename: string,
  sourcePath: string | null,
): BufferDocument {
  return {
    id,
    title: filename,
    filename,
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
  };
}

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

let capturedLanguage: (() => string | null) | undefined;

function Probe() {
  const win = useWindow();
  capturedLanguage = win.editor.language;
  return null;
}

async function mount(initial: BufferDocument) {
  const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
  const [buf] = createSignal(initial);
  render(() => (
    <WindowProvider windowId={9401}>
      <Probe />
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return capturedLanguage!();
}

describe("EditorInstance: note language stays markdown", () => {
  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
    capturedLanguage = undefined;
  });

  afterEach(() => {
    cleanup();
  });

  it("keeps an untitled note with a fenced shell block as markdown and registers the format commands", async () => {
    bufferContent.set("N1", "```sh\necho hi\n```\n");
    const language = await mount(mockBuffer("N1", "a1b2c3.txt", null));

    expect(language).toBe("markdown");
    expect(getCommand("editor.toggleBold")).toBeDefined();
  });

  it("honors a shebang on an untitled note", async () => {
    bufferContent.set("N2", "#!/bin/sh\necho hi\n");
    const language = await mount(mockBuffer("N2", "d4e5f6.txt", null));

    expect(language).toBe("shell");
  });

  it("leaves an on-disk shell file detected as shell", async () => {
    bufferContent.set("F1", "```sh\nnot actually shell content\n```\n");
    const language = await mount(mockBuffer("F1", "foo.sh", "/tmp/foo.sh"));

    expect(language).toBe("shell");
  });

  it("reports an empty untitled note as markdown", async () => {
    bufferContent.set("N3", "");
    const language = await mount(mockBuffer("N3", "g7h8i9.txt", null));

    expect(language).toBe("markdown");
    expect(getCommand("editor.toggleBold")).toBeDefined();
  });

  // Deliberate boundary: a user-typed filename extension is a rename signal,
  // not content-sniffing, so it still overrides the note-is-markdown default
  // (see editor-instance-chrome.test.tsx's rename tests). Only untyped
  // default names (uuid.txt) and content heuristics are forced to markdown.
  it("still honors an explicit code extension in an untitled note's title", async () => {
    bufferContent.set("N4", "some prose that is not code\n");
    const language = await mount(mockBuffer("N4", "main.rs", null));

    expect(language).toBe("rust");
    expect(getCommand("editor.toggleBold")).toBeUndefined();
  });
});
