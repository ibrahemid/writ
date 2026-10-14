import { describe, it, expect, vi, beforeEach } from "vitest";

const h = vi.hoisted(() => ({
  noteNameCandidates: vi.fn(async () => [] as { path: string; name: string; folder: string }[]),
  notePathsInFolder: vi.fn(async () => [] as string[]),
  active: [] as { source_path: string | null }[],
  history: [] as { source_path: string | null; closed_at: string | null; updated_at: string; created_at: string }[],
}));

vi.mock("../../services/tauri", () => ({
  noteNameCandidates: h.noteNameCandidates,
  notePathsInFolder: h.notePathsInFolder,
}));
vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: { activeTabs: () => h.active, historyList: () => h.history },
}));
vi.mock("../../stores/global/notes", () => ({ notesStore: { root: () => "/n" } }));
vi.mock("../../components/Notifications/Toast", () => ({ showToast: vi.fn() }));

import { linkStore } from "../../stores/global/link";

beforeEach(() => {
  vi.clearAllMocks();
  h.active = [];
  h.history = [];
});

function closed(path: string, at: string) {
  return { source_path: path, closed_at: at, updated_at: at, created_at: at };
}

describe("the files a bare [[ offers", () => {
  it("lists open and recently closed files, then the ones beside this file", async () => {
    h.active = [{ source_path: "/n/work/today.md" }, { source_path: "/n/plan.md" }];
    h.history = [closed("/n/old.md", "2026-10-01T00:00:00Z"), closed("/n/newer.md", "2026-10-02T00:00:00Z")];
    h.notePathsInFolder.mockResolvedValue(["/n/work/retro.md", "/n/work/today.md"]);

    const links = await linkStore.linkCandidates("", "/n/work/today.md");

    expect(h.notePathsInFolder).toHaveBeenCalledWith("work");
    expect(h.noteNameCandidates).not.toHaveBeenCalled();
    expect(links.map((link) => link.name)).toEqual(["plan", "newer", "old", "retro"]);
  });

  it("asks for no folder listing for a file at the top of the notes folder", async () => {
    h.active = [{ source_path: "/n/plan.md" }];
    await linkStore.linkCandidates("", "/n/today.md");
    expect(h.notePathsInFolder).not.toHaveBeenCalled();
  });

  it("leaves a typed query to the index's ranking", async () => {
    h.history = [closed("/n/b/plan.md", "2026-10-01T00:00:00Z")];
    const ranked = [
      { path: "/n/a/plan.md", name: "plan", folder: "a" },
      { path: "/n/b/plan.md", name: "plan", folder: "b" },
      { path: "/n/work/planning.md", name: "planning", folder: "work" },
    ];
    h.noteNameCandidates.mockResolvedValue(ranked);

    const links = await linkStore.linkCandidates("plan", "/n/work/today.md");
    expect(h.noteNameCandidates).toHaveBeenCalledWith("plan", undefined);
    expect(links).toEqual(ranked);
    expect(h.notePathsInFolder).not.toHaveBeenCalled();
  });
});
