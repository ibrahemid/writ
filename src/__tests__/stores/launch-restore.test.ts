import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { BufferDocument, FileOpenResult } from "../../types/buffer";

// The tabs the last session left open come back at launch, and each file is
// measured before anything reads it. A file over the size Writ opens without
// asking waits unread until its tab is shown, then answers as reopening it
// from History does: the question with its size, and the read only after Open.
// A file past the size Writ opens keeps its tab and says why.

const SIXTY_MB = 60_000_000;
const SIXTY_ONE_MB = 61_000_000;
const SIX_HUNDRED_MB = 600_000_000;
const FIFTY_MIB = 50 * 1024 * 1024;

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

const small = row("small", "small.log");
const big = row("big", "big.log", { size_bytes: SIXTY_MB });
const huge = row("huge", "huge.log", { size_bytes: SIX_HUNDRED_MB });
const atTheLine = row("line", "line.log", { size_bytes: FIFTY_MIB });
const hexView = row("hex", "image.bin", { size_bytes: SIXTY_MB, read_only: true });
const untitled = row("untitled", "Untitled", { source_path: null, size_bytes: SIXTY_MB });
const ALL_IDS = [small, big, huge, atTheLine, hexView, untitled].map((doc) => doc.id);

let restored: BufferDocument[] = [];

vi.mock("../../services/tauri", () => ({
  listActiveBuffers: vi.fn(async () => restored),
  listHistory: vi.fn(async () => []),
  getBuffer: vi.fn(),
  restoreBuffer: vi.fn(),
  restoreBufferConfirmed: vi.fn(),
  openFile: vi.fn(),
  openFileConfirmed: vi.fn(),
  todaysNote: vi.fn(),
  readBufferContent: vi.fn(async () => "a line of a log\n"),
  closeBuffer: vi.fn(async () => undefined),
  previewClose: vi.fn(async () => undefined),
}));

vi.mock("../../services/autosave", () => ({
  flushAutosave: vi.fn(async () => ({ ok: true, failures: [] })),
  keepUnsavedForRecovery: vi.fn(async () => undefined),
  cancelAutosave: vi.fn(),
  onAutosaveStart: vi.fn(() => () => {}),
  onAutosaveSuccess: vi.fn(() => () => {}),
  onAutosaveError: vi.fn(() => () => {}),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import * as api from "../../services/tauri";
import { showToast } from "../../components/Notifications/Toast";
import { bufferRegistry, UnreadTabReadError } from "../../stores/global/buffer-registry";
import { unreadTabs } from "../../stores/global/unread-tabs";
import { largeFileOpen } from "../../stores/global/large-file-open";
import { createTabStore } from "../../stores/window/tab-store";
import type { DownloadStore } from "../../stores/window/download-store";

const mockedApi = vi.mocked(api);

function answerMeasure(result: FileOpenResult): void {
  mockedApi.restoreBuffer.mockResolvedValue(result as never);
}

function answerConfirmed(result: FileOpenResult): void {
  mockedApi.restoreBufferConfirmed.mockResolvedValue(result as never);
}

function tabStore() {
  const downloads = {
    select: vi.fn(),
    start: vi.fn(async () => undefined),
  } as unknown as DownloadStore;
  return createTabStore({ registry: bufferRegistry, downloads });
}

// Answers every question still up or waiting with Cancel, and ends any
// progress, so no test leaves one behind for the next.
function drain(): void {
  for (let i = 0; i < 20 && largeFileOpen.state() !== null; i++) {
    largeFileOpen.respond(false);
    largeFileOpen.finish();
  }
}

beforeEach(() => {
  vi.clearAllMocks();
  restored = [small, big, huge, atTheLine, hexView, untitled];
});

afterEach(() => {
  drain();
  for (const id of ALL_IDS) unreadTabs.clear(id);
});

describe("the session Writ restores at launch", () => {
  it("keeps a tab whose file is over 50 MiB unread, with its size", async () => {
    await bufferRegistry.loadSession();

    expect(unreadTabs.stateOf(big.id)).toEqual({ kind: "waiting", sizeBytes: SIXTY_MB });
    await expect(bufferRegistry.readContent(big.id)).rejects.toThrow();
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
  });

  it("keeps a tab whose file is past the size Writ opens unread, with the reason", async () => {
    await bufferRegistry.loadSession();

    expect(unreadTabs.stateOf(huge.id)).toEqual({
      kind: "refused",
      reason: "it is 600 MB, and Writ opens files up to 524 MB.",
    });
    await expect(bufferRegistry.readContent(huge.id)).rejects.toThrow();
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
  });

  it("reads at once a file at or under 50 MiB, a hex view under the limit, or a note with no file", async () => {
    await bufferRegistry.loadSession();

    for (const doc of [small, atTheLine, hexView, untitled]) {
      expect(unreadTabs.stateOf(doc.id), doc.title).toBeNull();
    }
    await expect(bufferRegistry.readContent(small.id)).resolves.toBe("a line of a log\n");
  });

  it("leaves unread nothing a later load finds", async () => {
    restored = [small];
    await bufferRegistry.loadSession();

    restored = [small, big];
    await bufferRegistry.load();

    expect(unreadTabs.stateOf(big.id)).toBeNull();
  });
});

describe("an unread tab, when it is shown", () => {
  it("asks with the size its file has now, then opens through the confirmed reopen", async () => {
    await bufferRegistry.loadSession();
    answerMeasure({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_ONE_MB });
    answerConfirmed({
      doc: { ...big, size_bytes: SIXTY_ONE_MB },
      mode: { kind: "LargeFile" },
      size_bytes: SIXTY_ONE_MB,
    });
    const tabs = tabStore();
    tabs.setActiveTabId(big.id);

    const answer = tabs.openUnreadTab(big.id);

    await vi.waitFor(() =>
      expect(largeFileOpen.state()).toEqual({
        phase: "asking",
        name: "big.log",
        sizeBytes: SIXTY_ONE_MB,
      }),
    );
    expect(mockedApi.restoreBufferConfirmed).not.toHaveBeenCalled();
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();

    largeFileOpen.respond(true);
    const outcome = await answer;

    expect(outcome.kind).toBe("opened");
    expect(mockedApi.restoreBufferConfirmed).toHaveBeenCalledWith(big.id);
    expect(unreadTabs.stateOf(big.id)).toBeNull();
    expect(bufferRegistry.buffers().find((b) => b.id === big.id)?.size_bytes).toBe(SIXTY_ONE_MB);
    expect(tabs.activeTabId()).toBe(big.id);
    // The progress stays up until the editor shows the text.
    expect(largeFileOpen.state()).toMatchObject({ phase: "opening", bufferId: big.id });
    await expect(bufferRegistry.readContent(big.id)).resolves.toBe("a line of a log\n");
  });

  it("stays unread and open after Cancel, and says nothing", async () => {
    await bufferRegistry.loadSession();
    answerMeasure({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_ONE_MB });
    const tabs = tabStore();
    tabs.setActiveTabId(big.id);

    const answer = tabs.openUnreadTab(big.id);
    await vi.waitFor(() => expect(largeFileOpen.state()?.phase).toBe("asking"));
    largeFileOpen.respond(false);
    const outcome = await answer;

    expect(outcome.kind).toBe("declined");
    expect(unreadTabs.stateOf(big.id)).toEqual({ kind: "waiting", sizeBytes: SIXTY_ONE_MB });
    expect(bufferRegistry.activeTabs().some((b) => b.id === big.id)).toBe(true);
    expect(tabs.activeTabId()).toBe(big.id);
    expect(mockedApi.closeBuffer).not.toHaveBeenCalled();
    expect(mockedApi.restoreBufferConfirmed).not.toHaveBeenCalled();
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it("becomes a refusal in its tab when the file grew past the limit, with no toast", async () => {
    await bufferRegistry.loadSession();
    answerMeasure({
      doc: null,
      mode: { kind: "Refused", reason: "ERR_OPEN_TOO_LARGE: 572.2 MiB is over the 500.0 MiB limit" },
      size_bytes: SIX_HUNDRED_MB,
    });
    const tabs = tabStore();

    const outcome = await tabs.openUnreadTab(big.id);

    expect(outcome.kind).toBe("refused");
    expect(unreadTabs.stateOf(big.id)).toEqual({
      kind: "refused",
      reason: "it is 600 MB, and Writ opens files up to 524 MB.",
    });
    expect(largeFileOpen.state()).toBeNull();
    expect(showToast).not.toHaveBeenCalled();
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
  });

  it("opens at once when the file shrank under 50 MiB", async () => {
    await bufferRegistry.loadSession();
    answerMeasure({
      doc: { ...big, size_bytes: 2_000 },
      mode: { kind: "Normal" },
      size_bytes: 2_000,
    });
    const tabs = tabStore();

    const outcome = await tabs.openUnreadTab(big.id);

    expect(outcome.kind).toBe("opened");
    expect(largeFileOpen.state()).toBeNull();
    expect(unreadTabs.stateOf(big.id)).toBeNull();
  });

  it("asks once however often the tab is shown while the question is up", async () => {
    await bufferRegistry.loadSession();
    answerMeasure({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB });
    const tabs = tabStore();

    const first = tabs.openUnreadTab(big.id);
    const second = tabs.openUnreadTab(big.id);
    await vi.waitFor(() => expect(largeFileOpen.state()?.phase).toBe("asking"));
    largeFileOpen.respond(false);

    expect((await first).kind).toBe("declined");
    expect((await second).kind).toBe("declined");
    expect(mockedApi.restoreBuffer).toHaveBeenCalledTimes(1);
    expect(largeFileOpen.state()).toBeNull();
  });

  it("measures a refused tab again when it is asked to", async () => {
    await bufferRegistry.loadSession();
    answerMeasure({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB });
    const tabs = tabStore();

    const answer = tabs.openUnreadTab(huge.id);
    await vi.waitFor(() => expect(largeFileOpen.state()?.phase).toBe("asking"));

    expect(mockedApi.restoreBuffer).toHaveBeenCalledWith(huge.id);
    expect(unreadTabs.stateOf(huge.id)).toEqual({ kind: "waiting", sizeBytes: SIXTY_MB });
    largeFileOpen.respond(false);
    await answer;
  });
});

// An open of the path an unread tab shows (the tree, the palette, a drop) is
// that tab's open: it asks, and Cancel answers declined and moves nothing.
describe("an open of an unread tab's path", () => {
  it("asks first, and Cancel leaves the selection where it was", async () => {
    await bufferRegistry.loadSession();
    answerMeasure({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB });
    const tabs = tabStore();
    tabs.setActiveTabId(small.id);

    const answer = tabs.openPath(big.source_path!);
    await vi.waitFor(() => expect(largeFileOpen.state()?.phase).toBe("asking"));
    largeFileOpen.respond(false);
    const outcome = await answer;

    expect(outcome.kind).toBe("declined");
    expect(tabs.activeTabId()).toBe(small.id);
    expect(unreadTabs.stateOf(big.id)).toEqual({ kind: "waiting", sizeBytes: SIXTY_MB });
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
  });

  it("brings the tab in front once Open answered", async () => {
    await bufferRegistry.loadSession();
    answerMeasure({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB });
    answerConfirmed({ doc: big, mode: { kind: "LargeFile" }, size_bytes: SIXTY_MB });
    const tabs = tabStore();
    tabs.setActiveTabId(small.id);

    const answer = tabs.openPath(big.source_path!);
    await vi.waitFor(() => expect(largeFileOpen.state()?.phase).toBe("asking"));
    largeFileOpen.respond(true);
    const outcome = await answer;

    expect(outcome.kind).toBe("opened");
    expect(tabs.activeTabId()).toBe(big.id);
    expect(unreadTabs.stateOf(big.id)).toBeNull();
  });
});

describe("closing an unread tab", () => {
  it("leaves the unread set, and its file is measured again from History", async () => {
    await bufferRegistry.loadSession();
    const tabs = tabStore();

    await tabs.closeTab(big.id);

    expect(mockedApi.closeBuffer).toHaveBeenCalledWith(big.id);
    expect(unreadTabs.stateOf(big.id)).toBeNull();
  });
});

// A path that spells an unread tab's file another way (a link, a case variant
// in a link) does not match the tab's path here, so it reaches Rust, which
// answers with the tab's row and reads nothing. The tab's own open answers it.
describe("an open of an unread tab's file by another spelling", () => {
  const link = "/Users/me/logs/link-to-big.log";

  function rustAnswersTheTab(): void {
    mockedApi.openFile.mockResolvedValue({
      doc: big,
      mode: { kind: "LargeFile" },
      size_bytes: SIXTY_MB,
    } as never);
  }

  it("asks with the tab's file, and Cancel leaves the tab unread", async () => {
    await bufferRegistry.loadSession();
    rustAnswersTheTab();
    answerMeasure({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB });
    const tabs = tabStore();
    tabs.setActiveTabId(small.id);

    const answer = tabs.openPath(link);
    await vi.waitFor(() =>
      expect(largeFileOpen.state()).toEqual({
        phase: "asking",
        name: "big.log",
        sizeBytes: SIXTY_MB,
      }),
    );
    largeFileOpen.respond(false);
    const outcome = await answer;

    expect(outcome.kind).toBe("declined");
    expect(mockedApi.restoreBuffer).toHaveBeenCalledWith(big.id);
    expect(unreadTabs.stateOf(big.id)).toEqual({ kind: "waiting", sizeBytes: SIXTY_MB });
    expect(tabs.activeTabId()).toBe(small.id);
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
  });

  it("answers opened only once the tab's open did, and the tab is read after", async () => {
    await bufferRegistry.loadSession();
    rustAnswersTheTab();
    answerMeasure({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB });
    answerConfirmed({ doc: big, mode: { kind: "LargeFile" }, size_bytes: SIXTY_MB });
    const tabs = tabStore();

    const answer = tabs.openPath(link);
    await vi.waitFor(() => expect(largeFileOpen.state()?.phase).toBe("asking"));
    largeFileOpen.respond(true);
    const outcome = await answer;

    expect(outcome.kind).toBe("opened");
    expect(mockedApi.restoreBufferConfirmed).toHaveBeenCalledWith(big.id);
    expect(unreadTabs.stateOf(big.id)).toBeNull();
    expect(tabs.activeTabId()).toBe(big.id);
  });

  // Rust answered from the size the row recorded; the tab's own open measures
  // the file now, and what it finds is the answer, kept in the tab.
  it("takes what the tab's own measure finds, a refusal included", async () => {
    await bufferRegistry.loadSession();
    rustAnswersTheTab();
    answerMeasure({
      doc: null,
      mode: { kind: "Refused", reason: "ERR_OPEN_TOO_LARGE: 572.2 MiB is over the 500.0 MiB limit" },
      size_bytes: SIX_HUNDRED_MB,
    });
    const tabs = tabStore();

    const outcome = await tabs.openPath(link);

    expect(outcome.kind).toBe("refused");
    expect(unreadTabs.stateOf(big.id)).toEqual({
      kind: "refused",
      reason: "it is 600 MB, and Writ opens files up to 524 MB.",
    });
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
  });

  it("is Today's File's answer too when today's file is an unread tab", async () => {
    await bufferRegistry.loadSession();
    mockedApi.todaysNote.mockResolvedValue({
      path: big.source_path!,
      opened: { doc: big, mode: { kind: "LargeFile" }, size_bytes: SIXTY_MB },
    } as never);
    answerMeasure({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: SIXTY_MB });
    const tabs = tabStore();
    tabs.setActiveTabId(small.id);

    const answer = tabs.todaysNote();
    await vi.waitFor(() => expect(largeFileOpen.state()?.phase).toBe("asking"));
    largeFileOpen.respond(false);

    expect(await answer).toBeNull();
    expect(mockedApi.restoreBuffer).toHaveBeenCalledWith(big.id);
    expect(unreadTabs.stateOf(big.id)).toEqual({ kind: "waiting", sizeBytes: SIXTY_MB });
    expect(tabs.activeTabId()).toBe(small.id);
  });
});

// The launch measured a file under the size Writ opens and it grew past it
// before anything read it. Rust measures before it reads, records the size it
// found on the row and answers with the refusal's code instead of the text.
describe("a read Rust declines for the size", () => {
  const tooLarge = "ERR_OPEN_TOO_LARGE: the file is 600000000 bytes, over the size Writ opens";

  it("leaves the tab unread, refused with the size on its row", async () => {
    restored = [small];
    await bufferRegistry.loadSession();
    mockedApi.readBufferContent.mockRejectedValueOnce(tooLarge);
    mockedApi.getBuffer.mockResolvedValue({ ...small, size_bytes: SIX_HUNDRED_MB } as never);

    await expect(bufferRegistry.readContent(small.id)).rejects.toBeInstanceOf(UnreadTabReadError);

    expect(mockedApi.getBuffer).toHaveBeenCalledWith(small.id);
    expect(unreadTabs.stateOf(small.id)).toEqual({
      kind: "refused",
      reason: "it is 600 MB, and Writ opens files up to 524 MB.",
    });
    expect(bufferRegistry.buffers().find((b) => b.id === small.id)?.size_bytes).toBe(
      SIX_HUNDRED_MB,
    );
    mockedApi.readBufferContent.mockClear();
    await expect(bufferRegistry.readContent(small.id)).rejects.toBeInstanceOf(UnreadTabReadError);
    expect(mockedApi.readBufferContent).not.toHaveBeenCalled();
  });

  it("names the limit when the row with the size could not be read", async () => {
    restored = [small];
    await bufferRegistry.loadSession();
    mockedApi.readBufferContent.mockRejectedValueOnce(tooLarge);
    mockedApi.getBuffer.mockRejectedValue("ERR_READ_FAILED: database is locked");

    await expect(bufferRegistry.readContent(small.id)).rejects.toBeInstanceOf(UnreadTabReadError);

    expect(unreadTabs.stateOf(small.id)).toEqual({
      kind: "refused",
      reason: "it is over the 524 MB limit.",
    });
  });

  it("passes any other failure through as it came", async () => {
    restored = [small];
    await bufferRegistry.loadSession();
    mockedApi.readBufferContent.mockRejectedValueOnce("ERR_READ_FILE_MISSING: gone");

    await expect(bufferRegistry.readContent(small.id)).rejects.toBe("ERR_READ_FILE_MISSING: gone");

    expect(unreadTabs.stateOf(small.id)).toBeNull();
    expect(mockedApi.getBuffer).not.toHaveBeenCalled();
  });
});
