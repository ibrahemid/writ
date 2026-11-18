import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { DropPoint } from "../../types/image-import";

const mocks = vi.hoisted(() => ({
  importDroppedImages: vi.fn(),
  pasteClipboardImages: vi.fn(),
  previewRemoteImage: vi.fn(),
  showToast: vi.fn(),
  logFailure: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  importDroppedImages: mocks.importDroppedImages,
  pasteClipboardImages: mocks.pasteClipboardImages,
  previewRemoteImage: mocks.previewRemoteImage,
}));

vi.mock("../../stores/global/notifications", () => ({
  showToast: mocks.showToast,
}));

vi.mock("../../lib/log", () => ({
  logFailure: mocks.logFailure,
}));

import {
  DROP_FAILED_NOTICE,
  DROP_TAB_CHANGED_NOTICE,
  imageDropTargetFor,
  loadRemoteImage,
  pasteClipboardImages,
  registerImageDropTarget,
  showImageNotices,
  takeDroppedImages,
  type ImageDropTarget,
} from "../../stores/global/images";

interface FakeTarget extends ImageDropTarget {
  inserted: { markdown: string; point: DropPoint | null; bufferId: string }[];
}

function fakeTarget(options: {
  bufferId?: string;
  acceptsImages?: boolean;
  box?: { left: number; top: number; right: number; bottom: number };
  focused?: boolean;
  holds?: boolean;
}): FakeTarget {
  const box = options.box ?? { left: 0, top: 0, right: 100, bottom: 100 };
  const inserted: FakeTarget["inserted"] = [];
  return {
    inserted,
    bufferId: () => options.bufferId ?? "buf-1",
    acceptsImages: () => options.acceptsImages ?? true,
    contains: (point) =>
      point.x >= box.left && point.x < box.right && point.y >= box.top && point.y < box.bottom,
    hasFocus: () => options.focused ?? false,
    insert: (markdown, point, bufferId) => {
      if (options.holds === false) return false;
      inserted.push({ markdown, point, bufferId });
      return true;
    },
  };
}

const removals: (() => void)[] = [];

function register(target: ImageDropTarget): ImageDropTarget {
  removals.push(registerImageDropTarget(target));
  return target;
}

beforeEach(() => {
  mocks.importDroppedImages.mockReset();
  mocks.pasteClipboardImages.mockReset();
  mocks.previewRemoteImage.mockReset();
  mocks.showToast.mockReset();
  mocks.logFailure.mockReset();
});

afterEach(() => {
  while (removals.length > 0) removals.pop()!();
});

describe("imageDropTargetFor", () => {
  it("finds the editor under the drop point", () => {
    register(fakeTarget({ bufferId: "left", box: { left: 0, top: 0, right: 50, bottom: 100 } }));
    const right = register(
      fakeTarget({ bufferId: "right", box: { left: 50, top: 0, right: 100, bottom: 100 } }),
    );
    expect(imageDropTargetFor({ x: 75, y: 10 })).toBe(right);
  });

  it("uses the focused editor when the drop has no point", () => {
    register(fakeTarget({ bufferId: "a" }));
    const focused = register(fakeTarget({ bufferId: "b", focused: true }));
    expect(imageDropTargetFor(null)).toBe(focused);
  });

  it("answers null when the editor under the point takes no images", () => {
    register(fakeTarget({ acceptsImages: false }));
    expect(imageDropTargetFor({ x: 10, y: 10 })).toBeNull();
  });

  it("answers null when no editor is under the point", () => {
    register(fakeTarget({}));
    expect(imageDropTargetFor({ x: 500, y: 500 })).toBeNull();
  });

  it("forgets an editor once it is removed", () => {
    const remove = registerImageDropTarget(fakeTarget({}));
    remove();
    expect(imageDropTargetFor({ x: 10, y: 10 })).toBeNull();
  });
});

describe("takeDroppedImages", () => {
  it("opens every path when the drop is not on a Markdown editor", async () => {
    register(fakeTarget({ acceptsImages: false }));
    const paths = ["/tmp/a.png", "/tmp/b.txt"];
    expect(await takeDroppedImages(paths, { x: 10, y: 10 })).toEqual(paths);
    expect(mocks.importDroppedImages).not.toHaveBeenCalled();
  });

  it("inserts the references at the drop point and opens the other files", async () => {
    const target = register(fakeTarget({ bufferId: "buf-7" })) as FakeTarget;
    mocks.importDroppedImages.mockResolvedValue({
      markdown: "![](assets/a.png)",
      others: ["/tmp/b.txt"],
      notices: [],
    });
    const rest = await takeDroppedImages(["/tmp/a.png", "/tmp/b.txt"], { x: 10, y: 20 });
    expect(mocks.importDroppedImages).toHaveBeenCalledWith("buf-7", ["/tmp/a.png", "/tmp/b.txt"]);
    expect(target.inserted).toEqual([
      { markdown: "![](assets/a.png)", point: { x: 10, y: 20 }, bufferId: "buf-7" },
    ]);
    expect(rest).toEqual(["/tmp/b.txt"]);
    expect(mocks.showToast).not.toHaveBeenCalled();
  });

  it("shows a notice for each image that was not taken", async () => {
    register(fakeTarget({ focused: true }));
    mocks.importDroppedImages.mockResolvedValue({
      markdown: "",
      others: [],
      notices: ["big.png is over the 16 MB limit for images"],
    });
    const rest = await takeDroppedImages(["/tmp/big.png"], null);
    expect(rest).toEqual([]);
    expect(mocks.showToast).toHaveBeenCalledWith("big.png is over the 16 MB limit for images", "warning", 6000);
  });

  it("says so when the editor moved to another tab before the insert", async () => {
    register(fakeTarget({ holds: false }));
    mocks.importDroppedImages.mockResolvedValue({
      markdown: "![](assets/a.png)",
      others: [],
      notices: [],
    });
    await takeDroppedImages(["/tmp/a.png"], { x: 1, y: 1 });
    expect(mocks.showToast).toHaveBeenCalledWith(DROP_TAB_CHANGED_NOTICE, "warning", 6000);
  });

  it("opens every path and logs when the host fails", async () => {
    register(fakeTarget({}));
    mocks.importDroppedImages.mockRejectedValue(new Error("stopped"));
    const paths = ["/tmp/a.png"];
    expect(await takeDroppedImages(paths, { x: 1, y: 1 })).toEqual(paths);
    expect(mocks.showToast).toHaveBeenCalledWith(DROP_FAILED_NOTICE, "error");
    expect(mocks.logFailure).toHaveBeenCalledOnce();
  });

  it("answers an empty drop as it is", async () => {
    register(fakeTarget({}));
    expect(await takeDroppedImages([], { x: 1, y: 1 })).toEqual([]);
    expect(mocks.importDroppedImages).not.toHaveBeenCalled();
  });
});

describe("host calls", () => {
  it("pastes the clipboard into the tab it names", async () => {
    mocks.pasteClipboardImages.mockResolvedValue({ kind: "empty" });
    expect(await pasteClipboardImages("buf-3")).toEqual({ kind: "empty" });
    expect(mocks.pasteClipboardImages).toHaveBeenCalledWith("buf-3");
  });

  it("loads a web image for the tab it names", async () => {
    mocks.previewRemoteImage.mockResolvedValue({ src: "jotra-preview://x" });
    expect(await loadRemoteImage("buf-3", "https://e.com/a.png", "automatic")).toEqual({
      src: "jotra-preview://x",
    });
    expect(mocks.previewRemoteImage).toHaveBeenCalledWith(
      "buf-3",
      "https://e.com/a.png",
      "automatic",
    );
  });

  it("runs at most four unasked web image loads at once", async () => {
    const answers: { resolve: (value: { src: string }) => void; reject: (e: Error) => void }[] =
      [];
    mocks.previewRemoteImage.mockImplementation(
      () =>
        new Promise((resolve, reject) => {
          answers.push({ resolve, reject });
        }),
    );
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    const loads = [1, 2, 3, 4, 5, 6].map((n) =>
      loadRemoteImage("buf-1", `https://e.com/${n}.png`, "automatic"),
    );
    loads.forEach((load) => load.catch(() => {}));
    await settle();
    expect(mocks.previewRemoteImage).toHaveBeenCalledTimes(4);

    const asked = loadRemoteImage("buf-1", "https://e.com/click.png", "asked");
    await settle();
    expect(mocks.previewRemoteImage).toHaveBeenCalledTimes(5);
    expect(mocks.previewRemoteImage).toHaveBeenLastCalledWith(
      "buf-1",
      "https://e.com/click.png",
      "asked",
    );

    answers[0].resolve({ src: "jotra-preview://1" });
    await settle();
    expect(await loads[0]).toEqual({ src: "jotra-preview://1" });
    expect(mocks.previewRemoteImage).toHaveBeenCalledTimes(6);

    answers[1].reject(new Error("refused"));
    await settle();
    await expect(loads[1]).rejects.toThrow("refused");
    expect(mocks.previewRemoteImage).toHaveBeenCalledTimes(7);
    expect(mocks.previewRemoteImage).toHaveBeenLastCalledWith(
      "buf-1",
      "https://e.com/6.png",
      "automatic",
    );

    for (const answer of answers.slice(2)) answer.resolve({ src: "jotra-preview://x" });
    await Promise.all([asked, ...loads.slice(2)]);
    mocks.previewRemoteImage.mockResolvedValue({ src: "jotra-preview://y" });
    await Promise.all(
      [1, 2, 3, 4, 5].map((n) => loadRemoteImage("buf-1", `https://e.com/again-${n}.png`, "automatic")),
    );
    expect(mocks.previewRemoteImage).toHaveBeenCalledTimes(12);
  });

  it("shows nothing for no notices", () => {
    showImageNotices([]);
    expect(mocks.showToast).not.toHaveBeenCalled();
  });
});
