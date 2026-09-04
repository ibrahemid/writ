import { describe, it, expect, vi, beforeEach } from "vitest";
import { createDownloadStore } from "../../stores/window/download-store";

const materialiseNote = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const cancelMaterialiseNote = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));

vi.mock("../../services/tauri", () => ({
  materialiseNote,
  cancelMaterialiseNote,
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => undefined),
}));

const NOTE = {
  path: "/home/user/Writ/away.md",
  title: "away.md",
  provider: "iCloud Drive",
};

describe("download store", () => {
  beforeEach(() => {
    materialiseNote.mockClear();
    cancelMaterialiseNote.mockClear();
  });

  it("asks for the bytes and shows the note as downloading", async () => {
    const downloads = createDownloadStore();
    await downloads.start(NOTE);

    expect(materialiseNote).toHaveBeenCalledWith(NOTE.path);
    expect(downloads.pending()).toEqual([
      { ...NOTE, state: "downloading", message: null },
    ]);
    expect(downloads.selected()?.path).toBe(NOTE.path);
  });

  it("opens the note once and drops the entry when the bytes arrive", async () => {
    const downloads = createDownloadStore();
    const reopen = vi.fn().mockResolvedValue(undefined);
    downloads.attachOpener(reopen);

    await downloads.start(NOTE);
    await downloads.handle({ path: NOTE.path, state: "done" });

    expect(reopen).toHaveBeenCalledTimes(1);
    expect(reopen).toHaveBeenCalledWith(NOTE.path, { activate: true });
    expect(downloads.pending()).toEqual([]);
    expect(downloads.selected()).toBeNull();
  });

  it("opens a note that finished behind another one without taking the screen", async () => {
    const downloads = createDownloadStore();
    const reopen = vi.fn().mockResolvedValue(undefined);
    downloads.attachOpener(reopen);

    await downloads.start(NOTE);
    // The person moved to another note while this one was downloading.
    downloads.select(null);
    await downloads.handle({ path: NOTE.path, state: "done" });

    expect(reopen).toHaveBeenCalledWith(NOTE.path, { activate: false });
    expect(downloads.pending()).toEqual([]);
  });

  it("opening the same note again joins the download rather than adding a second entry", async () => {
    const downloads = createDownloadStore();
    await downloads.start(NOTE);
    await downloads.start(NOTE);

    expect(downloads.pending()).toHaveLength(1);
    expect(materialiseNote).toHaveBeenCalledTimes(1);
  });

  it("cancelling calls the cancel command and opens nothing", async () => {
    const downloads = createDownloadStore();
    const reopen = vi.fn().mockResolvedValue(undefined);
    downloads.attachOpener(reopen);

    await downloads.start(NOTE);
    await downloads.cancel(NOTE.path);

    expect(cancelMaterialiseNote).toHaveBeenCalledWith(NOTE.path);
    expect(downloads.pending()).toEqual([]);
    expect(downloads.selected()).toBeNull();
    expect(reopen).not.toHaveBeenCalled();
  });

  it("a done event for a note that was cancelled opens nothing", async () => {
    const downloads = createDownloadStore();
    const reopen = vi.fn().mockResolvedValue(undefined);
    downloads.attachOpener(reopen);

    await downloads.start(NOTE);
    await downloads.cancel(NOTE.path);
    await downloads.handle({ path: NOTE.path, state: "done" });

    expect(reopen).not.toHaveBeenCalled();
  });

  it("keeps a failed note with what the provider said", async () => {
    const downloads = createDownloadStore();
    await downloads.start(NOTE);
    await downloads.handle({
      path: NOTE.path,
      state: "failed",
      message: "iCloud Drive is signed out",
    });

    expect(downloads.pending()).toEqual([
      { ...NOTE, state: "failed", message: "iCloud Drive is signed out" },
    ]);
  });

  it("keeps a note that timed out until it is closed", async () => {
    const downloads = createDownloadStore();
    await downloads.start(NOTE);
    await downloads.handle({ path: NOTE.path, state: "timed_out" });

    expect(downloads.pending()[0].state).toBe("timed_out");

    downloads.close(NOTE.path);
    expect(downloads.pending()).toEqual([]);
  });

  it("asks again for a note that stopped, without a second entry", async () => {
    const downloads = createDownloadStore();
    await downloads.start(NOTE);
    await downloads.handle({ path: NOTE.path, state: "timed_out" });
    await downloads.start(NOTE);

    expect(downloads.pending()).toHaveLength(1);
    expect(downloads.pending()[0].state).toBe("downloading");
    expect(materialiseNote).toHaveBeenCalledTimes(2);
  });

  it("reports a download that never started rather than waiting on it", async () => {
    materialiseNote.mockRejectedValueOnce(new Error("path not authorized"));
    const downloads = createDownloadStore();
    await downloads.start(NOTE);

    expect(downloads.pending()[0].state).toBe("failed");
    expect(downloads.pending()[0].message).toContain("path not authorized");
  });

  it("ignores an event for a note it is not waiting on", async () => {
    const downloads = createDownloadStore();
    const reopen = vi.fn().mockResolvedValue(undefined);
    downloads.attachOpener(reopen);

    await downloads.handle({ path: "/home/user/Writ/other.md", state: "done" });

    expect(reopen).not.toHaveBeenCalled();
    expect(downloads.pending()).toEqual([]);
  });

  it("names no provider when the folder is not in one", async () => {
    const downloads = createDownloadStore();
    await downloads.start({ ...NOTE, provider: null });

    expect(downloads.pending()[0].provider).toBeNull();
  });
});
