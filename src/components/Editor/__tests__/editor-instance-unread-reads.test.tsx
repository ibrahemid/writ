import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import { Show } from "solid-js";
import type { BufferDocument, FileOpenResult } from "../../../types/buffer";

// A file Writ has not read is not read by anything but its open: closing its
// tab reads nothing, and a file that grew past the size Writ opens after the
// launch measured it is refused by the read itself, in the tab and on the
// reload after a change outside Writ. These run the real registry, tab store
// and editor, mounted the way the editor area mounts them.

const SIXTY_MB = 60_000_000;
const SIX_HUNDRED_MB = 600_000_000;
const TOO_LARGE = "ERR_OPEN_TOO_LARGE: the file is 600000000 bytes, over the size Writ opens";
const REFUSED_LINE = "Could not open this file: it is 600 MB, and Writ opens files up to 524 MB.";

function row(id: string, name: string, overrides: Partial<BufferDocument> = {}): BufferDocument {
  return {
    id,
    title: name,
    filename: `${id}.txt`,
    status: "active",
    language: null,
    source_path: `/Users/me/logs/${name}`,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 1_000,
    line_ending: "lf",
    ...overrides,
  };
}

const small = row("r-small", "small.log");
const big = row("r-big", "big.log", { size_bytes: SIXTY_MB });
const huge = row("r-huge", "huge.log", { size_bytes: SIX_HUNDRED_MB });
const grown = row("r-grown", "grown.log");
const ALL_IDS = [small, big, huge, grown].map((doc) => doc.id);

let restored: BufferDocument[] = [];
const h = vi.hoisted(() => ({
  read: vi.fn(async (_id: string) => "2026-10-13 10:30:00 INFO a line of a log\n"),
  getBuffer: vi.fn(),
}));

vi.mock("../../../services/tauri", () => ({
  listActiveBuffers: vi.fn(async () => restored),
  listHistory: vi.fn(async () => []),
  getBuffer: (id: string) => h.getBuffer(id),
  restoreBuffer: vi.fn(),
  restoreBufferConfirmed: vi.fn(),
  openFile: vi.fn(),
  openFileConfirmed: vi.fn(),
  readBufferContent: (id: string) => h.read(id),
  saveBufferContent: vi.fn(async () => null),
  noteDiskState: vi.fn(async () => ({ state: "no_file" })),
  recordUnsavedNotes: vi.fn(async () => {}),
  closeBuffer: vi.fn(async () => undefined),
  closeBuffers: vi.fn(async () => undefined),
  previewClose: vi.fn(async () => undefined),
  deleteNote: vi.fn(async () => undefined),
  updateConfig: vi.fn(async () => {}),
}));

vi.mock("../../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import * as api from "../../../services/tauri";
import { bufferRegistry } from "../../../stores/global/buffer-registry";
import { unreadTabs } from "../../../stores/global/unread-tabs";
import { largeFileOpen } from "../../../stores/global/large-file-open";
import WindowProvider, { useWindow } from "../../WindowProvider/WindowProvider";
import type { WindowState } from "../../../stores/window/createWindowState";

const mockedApi = vi.mocked(api);

async function flush(n = 60): Promise<void> {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) {
    await flush();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

// The editor under a non-keyed Show on the tab in front, as PreviewLayout
// mounts it.
async function mountArea() {
  const EditorInstance = (await import("../EditorInstance")).default;
  let win!: WindowState;
  function Area() {
    win = useWindow();
    const active = () => {
      const id = win.tabs.activeTabId();
      if (!id) return null;
      return bufferRegistry.activeTabs().find((b) => b.id === id) ?? null;
    };
    return <Show when={active()}>{(buf) => <EditorInstance buffer={buf()} />}</Show>;
  }
  const rendered = render(() => (
    <WindowProvider windowId={9877}>
      <Area />
    </WindowProvider>
  ));
  await settle();
  return { ...rendered, win: () => win };
}

// Answers every question still up with Cancel and ends any progress, so no
// test leaves one behind for the next.
function drain(): void {
  for (let i = 0; i < 20 && largeFileOpen.state() !== null; i++) {
    largeFileOpen.respond(false);
    largeFileOpen.finish();
  }
}

const asks: FileOpenResult = {
  mode: { kind: "LargeFileConfirm" },
  size_bytes: SIXTY_MB,
  doc: null,
};

// The launch restores `small` and `front`, with `front` in front. A tab over
// 50 MiB asks as it is shown, and Cancel answers.
async function launchWithInFront(front: BufferDocument) {
  restored = [small, front];
  mockedApi.restoreBuffer.mockResolvedValue(asks);
  await bufferRegistry.loadSession();
  const mounted = await mountArea();
  mounted.win().tabs.setActiveTabId(front.id);
  await settle();
  if (largeFileOpen.state()?.phase === "asking") {
    largeFileOpen.respond(false);
    await settle();
  }
  return mounted;
}

function paneLine(container: HTMLElement): string | null | undefined {
  return container.querySelector(".note-read-failure-line")?.textContent;
}

beforeEach(() => {
  h.read.mockReset();
  h.read.mockImplementation(async () => "2026-10-13 10:30:00 INFO a line of a log\n");
  h.getBuffer.mockReset();
  mockedApi.restoreBuffer.mockReset();
  mockedApi.restoreBufferConfirmed.mockReset();
  mockedApi.openFile.mockReset();
});

afterEach(() => {
  cleanup();
  drain();
  for (const id of ALL_IDS) unreadTabs.clear(id);
});

describe("closing tabs with an unread tab in front", () => {
  it("Close all tabs reads nothing of a 60 MB tab the person cancelled", async () => {
    const { win, container } = await launchWithInFront(big);
    expect(paneLine(container)).toBe("This file is 60 MB and is not open.");
    h.read.mockClear();

    await win().tabs.closeAllTabs();
    await settle();

    expect(h.read).not.toHaveBeenCalled();
    expect(win().editor.currentBufferId()).toBeNull();
    expect(bufferRegistry.activeTabs()).toEqual([]);
    expect(unreadTabs.stateOf(big.id)).toBeNull();
  });

  it("Close all tabs reads nothing of a tab past the size Writ opens", async () => {
    const { win, container } = await launchWithInFront(huge);
    expect(paneLine(container)).toBe(REFUSED_LINE);
    h.read.mockClear();

    await win().tabs.closeAllTabs();
    await settle();

    expect(h.read).not.toHaveBeenCalled();
    expect(win().editor.currentBufferId()).toBeNull();
    expect(bufferRegistry.activeTabs()).toEqual([]);
  });

  it("closing several tabs without moving the selection reads nothing of the one in front", async () => {
    const { win } = await launchWithInFront(huge);
    h.read.mockClear();

    await bufferRegistry.closeBuffers([small.id, huge.id], { discard: true });
    await settle();

    expect(h.read).not.toHaveBeenCalled();
    expect(win().editor.currentBufferId()).toBeNull();
  });

  it("Close other tabs, Close tab and deleting the file read nothing of an unread tab", async () => {
    const others = await launchWithInFront(big);
    h.read.mockClear();
    await others.win().tabs.closeOtherTabs(small.id);
    await settle();
    expect(h.read).not.toHaveBeenCalledWith(big.id);
    cleanup();
    drain();

    const one = await launchWithInFront(big);
    h.read.mockClear();
    await one.win().tabs.closeTab(big.id);
    await settle();
    expect(h.read).not.toHaveBeenCalledWith(big.id);
    cleanup();
    drain();

    const deleted = await launchWithInFront(big);
    h.read.mockClear();
    await deleted.win().tabs.deleteNote(big.id);
    await settle();
    expect(h.read).not.toHaveBeenCalledWith(big.id);
  });
});

// The launch measured `grown` at 1,000 bytes; by the time it is read, its
// file is past the size Writ opens. Rust measures before it reads, records the
// size on the row and answers the refusal's code.
function growPastTheLimit(): void {
  h.read.mockImplementation(async (id: string) => {
    if (id === grown.id) throw TOO_LARGE;
    return "2026-10-13 10:30:00 INFO a line of a log\n";
  });
  h.getBuffer.mockImplementation(async (id: string) =>
    id === grown.id ? { ...grown, size_bytes: SIX_HUNDRED_MB } : small,
  );
}

describe("a tab whose file grew past the size Writ opens", () => {
  it("says why in its place when it is shown, with no read failure and no text", async () => {
    restored = [small, grown];
    await bufferRegistry.loadSession();
    const { win, container } = await mountArea();
    win().tabs.setActiveTabId(small.id);
    await settle();
    expect(container.querySelector(".cm-editor")).not.toBeNull();
    growPastTheLimit();

    win().tabs.setActiveTabId(grown.id);
    await settle();

    expect(paneLine(container)).toBe(REFUSED_LINE);
    expect(container.querySelectorAll(".note-read-failure")).toHaveLength(1);
    expect(container.querySelector(".cm-editor")).toBeNull();
    expect(win().editor.readFailure()).toBeNull();
    expect(win().editor.currentBufferId()).toBeNull();
    expect(unreadTabs.stateOf(grown.id)?.kind).toBe("refused");
    expect(bufferRegistry.buffers().find((b) => b.id === grown.id)?.size_bytes).toBe(
      SIX_HUNDRED_MB,
    );
    // Showing it again reads nothing: the tab is unread until its open answers.
    h.read.mockClear();
    win().tabs.setActiveTabId(small.id);
    await settle();
    win().tabs.setActiveTabId(grown.id);
    await settle();
    expect(h.read).not.toHaveBeenCalledWith(grown.id);
  });

  it("says why in its place on the reload after a change outside Writ", async () => {
    restored = [grown];
    await bufferRegistry.loadSession();
    const { win, container } = await mountArea();
    win().tabs.setActiveTabId(grown.id);
    await settle();
    expect(container.querySelector(".cm-content")?.textContent).toContain("a line of a log");
    growPastTheLimit();

    win().editor.requestExternalReload(grown.id);
    await settle();

    expect(paneLine(container)).toBe(REFUSED_LINE);
    expect(container.querySelector(".cm-editor")).toBeNull();
    expect(win().editor.currentBufferId()).toBeNull();
    expect(win().editor.liveDoc()).toBeNull();
  });

  // A log that keeps growing changes under its tab with every line. A tab
  // whose question was cancelled is not asked about again for each change.
  it("asks nothing again when a cancelled tab's file changes outside Writ", async () => {
    const { win, container } = await launchWithInFront(big);
    expect(mockedApi.restoreBuffer).toHaveBeenCalledTimes(1);
    h.read.mockClear();

    win().editor.requestExternalReload(big.id);
    await settle();

    expect(largeFileOpen.state()).toBeNull();
    expect(mockedApi.restoreBuffer).toHaveBeenCalledTimes(1);
    expect(h.read).not.toHaveBeenCalled();
    expect(paneLine(container)).toBe("This file is 60 MB and is not open.");
  });

  it("opens once Try again finds the file back under the size Writ opens without asking", async () => {
    restored = [grown];
    await bufferRegistry.loadSession();
    const { win, container, getByRole } = await mountArea();
    growPastTheLimit();
    win().tabs.setActiveTabId(grown.id);
    await settle();
    expect(paneLine(container)).toBe(REFUSED_LINE);

    const rotated = { ...grown, size_bytes: 2_000 };
    mockedApi.restoreBuffer.mockResolvedValue({
      mode: { kind: "Normal" },
      size_bytes: 2_000,
      doc: rotated,
    });
    h.read.mockImplementation(async () => "2026-10-13 10:31:00 INFO after the rotation\n");
    fireEvent.click(getByRole("button", { name: "Try again" }));
    await settle();

    expect(mockedApi.restoreBuffer).toHaveBeenCalledWith(grown.id);
    expect(unreadTabs.stateOf(grown.id)).toBeNull();
    expect(container.querySelector(".note-read-failure")).toBeNull();
    expect(container.querySelector(".cm-content")?.textContent).toContain("after the rotation");
    expect(win().editor.currentBufferId()).toBe(grown.id);
  });
});
