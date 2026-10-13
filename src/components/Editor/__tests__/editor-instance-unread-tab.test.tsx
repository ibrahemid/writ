import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import type { BufferDocument } from "../../../types/buffer";
import type { OpenOutcome } from "../../../stores/global/buffer-registry";
import WindowProvider from "../../WindowProvider/WindowProvider";

// A tab the last session left open whose file is over the size Writ opens
// without asking is not read when it is shown. It asks first, and its text is
// read only once the open answered. A file past the size Writ opens shows the
// reason in the editor's place.

const SIXTY_MB = 60_000_000;
const REFUSAL = "it is 600 MB, and Writ opens files up to 524 MB.";

const h = vi.hoisted(() => ({
  readContent: vi.fn(async (_id: string) => "2026-10-13 10:30:00 INFO a line of a log\n"),
  openUnreadTab: vi.fn(),
}));

vi.mock("../../../services/tauri", () => ({
  readBufferContent: vi.fn(h.readContent),
  saveBufferContent: vi.fn(async () => {}),
  updateConfig: vi.fn(async () => {}),
}));

vi.mock("../../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: h.readContent,
    openUnreadTab: h.openUnreadTab,
  },
}));

import { unreadTabs } from "../../../stores/global/unread-tabs";

function unreadRow(id: string, sizeBytes: number): BufferDocument {
  return {
    id,
    title: "server.log",
    filename: `${id}.txt`,
    status: "active",
    language: null,
    source_path: "/Users/me/logs/server.log",
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

async function flushMicrotasks(n = 10) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(buffer: BufferDocument) {
  const EditorInstance = (await import("../EditorInstance")).default;
  const [buf] = createSignal(buffer);
  const rendered = render(() => (
    <WindowProvider windowId={9811}>
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return rendered;
}

function opened(doc: BufferDocument): OpenOutcome {
  return { kind: "opened", doc, existed: true, mode: { kind: "LargeFile" } };
}

beforeEach(() => {
  h.readContent.mockClear();
  h.openUnreadTab.mockReset();
});

afterEach(() => {
  cleanup();
  for (const id of ["H1", "H2", "H3"]) unreadTabs.clear(id);
});

describe("EditorInstance: a tab restored unread at launch", () => {
  it("asks before anything reads it, and shows the file once the open answered", async () => {
    const doc = unreadRow("H1", SIXTY_MB);
    unreadTabs.takeRestored([doc]);
    let answer!: (outcome: OpenOutcome) => void;
    h.openUnreadTab.mockReturnValue(new Promise<OpenOutcome>((resolve) => (answer = resolve)));

    const { container } = await mount(doc);

    expect(h.openUnreadTab).toHaveBeenCalledWith("H1");
    expect(h.readContent).not.toHaveBeenCalled();
    expect(container.querySelector(".cm-editor")).toBeNull();

    unreadTabs.clear("H1");
    answer(opened(doc));
    await vi.waitFor(() => expect(container.querySelector(".cm-editor")).not.toBeNull());

    expect(h.readContent).toHaveBeenCalledWith("H1");
    expect(h.readContent).toHaveBeenCalledTimes(1);
    expect(container.querySelector(".cm-content")?.textContent).toContain("a line of a log");
    expect(container.querySelector(".note-read-failure")).toBeNull();
  });

  it("says the file is not open after Cancel, and asks again on Open…", async () => {
    const doc = unreadRow("H2", SIXTY_MB);
    unreadTabs.takeRestored([doc]);
    h.openUnreadTab.mockResolvedValue({ kind: "declined", path: doc.source_path! });

    const { container, getByRole } = await mount(doc);

    expect(container.querySelector(".note-read-failure-line")?.textContent).toBe(
      "This file is 60 MB and is not open.",
    );
    expect(h.openUnreadTab).toHaveBeenCalledTimes(1);
    expect(h.readContent).not.toHaveBeenCalled();

    fireEvent.click(getByRole("button", { name: "Open…" }));

    expect(h.openUnreadTab).toHaveBeenCalledTimes(2);
    expect(h.readContent).not.toHaveBeenCalled();
  });

  it("shows why a file past the limit is not open, and reads nothing", async () => {
    const doc = unreadRow("H3", 600_000_000);
    unreadTabs.takeRestored([doc]);
    h.openUnreadTab.mockResolvedValue({ kind: "refused", path: doc.source_path!, reason: REFUSAL });

    const { container, getByRole } = await mount(doc);

    expect(container.querySelector(".note-read-failure-line")?.textContent).toBe(
      `Could not open this file: ${REFUSAL}`,
    );
    expect(h.openUnreadTab).not.toHaveBeenCalled();
    expect(h.readContent).not.toHaveBeenCalled();
    expect(container.querySelector(".cm-editor")).toBeNull();

    fireEvent.click(getByRole("button", { name: "Try again" }));

    expect(h.openUnreadTab).toHaveBeenCalledWith("H3");
  });
});
