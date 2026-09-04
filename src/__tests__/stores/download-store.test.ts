import { describe, it, expect, vi, beforeEach } from "vitest";
import { createDownloadStore } from "../../stores/window/download-store";

const materialiseNote = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const cancelMaterialiseNote = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));

vi.mock("../../services/tauri", () => ({
  materialiseNote,
  cancelMaterialiseNote,
}));

const onEvent = vi.hoisted(() => vi.fn().mockResolvedValue(() => undefined));

vi.mock("../../services/events", () => ({ onEvent }));

const NOTE = {
  path: "/home/user/Writ/away.md",
  title: "away.md",
  provider: "iCloud Drive",
};

describe("download store", () => {
  beforeEach(() => {
    materialiseNote.mockClear();
    cancelMaterialiseNote.mockClear();
    onEvent.mockClear();
    onEvent.mockResolvedValue(() => undefined);
  });

  it("asks for the bytes and shows the note as downloading", async () => {
    const downloads = createDownloadStore();
    await downloads.start(NOTE);

    expect(materialiseNote).toHaveBeenCalledWith(NOTE.path);
    expect(downloads.pending()).toEqual([
      { ...NOTE, state: "downloading", reason: "download", message: null },
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
    // The open spends the permission itself; nothing hands it back.
    expect(cancelMaterialiseNote).not.toHaveBeenCalled();
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
      { ...NOTE, state: "failed", reason: "download", message: "iCloud Drive is signed out" },
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

  it("keeps the note when the bytes arrive and it still will not open", async () => {
    const downloads = createDownloadStore();
    downloads.attachOpener(vi.fn().mockRejectedValue(new Error("EACCES: permission denied")));

    await downloads.start(NOTE);
    await downloads.handle({ path: NOTE.path, state: "done" });

    expect(downloads.pending()).toHaveLength(1);
    expect(downloads.pending()[0].state).toBe("failed");
    expect(downloads.pending()[0].reason).toBe("open");
    // The failure is Writ's to explain, so nothing of the raw error is kept.
    expect(downloads.pending()[0].message).toBeNull();
    expect(downloads.selected()?.path).toBe(NOTE.path);
    // The tab is still up and the note can be opened again, so the permission
    // that open needs stays with it.
    expect(cancelMaterialiseNote).not.toHaveBeenCalled();
  });

  it("says so rather than waiting when nothing is listening for the download", async () => {
    onEvent.mockRejectedValueOnce(new Error("no ipc bridge"));
    const downloads = createDownloadStore();
    await downloads.mount().catch(() => undefined);

    await downloads.start(NOTE);

    expect(materialiseNote).not.toHaveBeenCalled();
    expect(downloads.pending()[0].state).toBe("failed");
    expect(downloads.pending()[0].reason).toBe("listener");
    expect(cancelMaterialiseNote).not.toHaveBeenCalled();
  });

  it("opens the note once when a second attempt gets the bytes", async () => {
    const downloads = createDownloadStore();
    const reopen = vi.fn().mockResolvedValue(undefined);
    downloads.attachOpener(reopen);

    await downloads.start(NOTE);
    await downloads.handle({ path: NOTE.path, state: "failed", message: "no space" });
    expect(downloads.pending()[0].state).toBe("failed");

    // The second attempt is the same tab asking again, not a second entry.
    await downloads.start(NOTE);
    expect(downloads.pending()).toHaveLength(1);
    expect(downloads.pending()[0].state).toBe("downloading");
    expect(materialiseNote).toHaveBeenCalledTimes(2);

    await downloads.handle({ path: NOTE.path, state: "done" });
    expect(reopen).toHaveBeenCalledTimes(1);
    expect(downloads.pending()).toEqual([]);
    expect(cancelMaterialiseNote).not.toHaveBeenCalled();
  });

  it("gives the permission back when a download that stopped is closed", async () => {
    const downloads = createDownloadStore();
    await downloads.start(NOTE);
    await downloads.handle({ path: NOTE.path, state: "failed", message: "no space" });

    await downloads.close(NOTE.path);

    expect(downloads.pending()).toEqual([]);
    expect(cancelMaterialiseNote).toHaveBeenCalledWith(NOTE.path);
  });

  it("gives the permission back when a second attempt is called off", async () => {
    const downloads = createDownloadStore();
    await downloads.start(NOTE);
    await downloads.handle({ path: NOTE.path, state: "timed_out" });
    expect(cancelMaterialiseNote).not.toHaveBeenCalled();

    await downloads.start(NOTE);
    await downloads.cancel(NOTE.path);

    expect(downloads.pending()).toEqual([]);
    expect(cancelMaterialiseNote).toHaveBeenCalledTimes(1);
    expect(cancelMaterialiseNote).toHaveBeenCalledWith(NOTE.path);
  });

  it("asks for the bytes only once the listener has attached", async () => {
    const order: string[] = [];
    let attach: (stop: () => void) => void = () => undefined;
    onEvent.mockReturnValueOnce(
      new Promise<() => void>((resolve) => {
        attach = (stop) => {
          order.push("listening");
          resolve(stop);
        };
      }),
    );
    materialiseNote.mockImplementationOnce(async () => {
      order.push("asked");
    });

    const downloads = createDownloadStore();
    void downloads.mount();
    const started = downloads.start(NOTE);
    attach(() => undefined);
    await started;

    expect(order).toEqual(["listening", "asked"]);
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
