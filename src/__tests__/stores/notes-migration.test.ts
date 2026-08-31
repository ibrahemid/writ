import { describe, it, expect, vi, beforeEach } from "vitest";

const h = vi.hoisted(() => ({
  getNotesMigrationReport: vi.fn(),
  dismissNotesMigrationReport: vi.fn(),
  moveArchivedNotes: vi.fn(),
  showNotesFolderInFinder: vi.fn(),
  showNotesFileInFileManager: vi.fn(),
  getNotesFolder: vi.fn(),
}));

vi.mock("../../services/tauri", () => h);

function report(overrides: Record<string, unknown> = {}) {
  return {
    ran_at: "2026-08-30T09:00:00Z",
    first_ran_at: "2026-08-30T09:00:00Z",
    notes_folder: "/home/user/Writ",
    archive_folder: "/home/user/.writ/archive",
    migrated: 3,
    archived: 2,
    recovered: 1,
    failed: 0,
    deleted_empty: 0,
    piped: 1,
    ...overrides,
  };
}

// The store is a singleton, and reading the report once per launch is the
// behaviour under test, so each test gets its own module instance.
async function freshStore() {
  vi.resetModules();
  const module = await import("../../stores/global/notes-migration");
  return module.notesMigrationStore;
}

describe("notes migration report store", () => {
  beforeEach(() => {
    h.getNotesMigrationReport.mockReset().mockResolvedValue(report());
    h.dismissNotesMigrationReport.mockReset().mockResolvedValue(undefined);
    h.moveArchivedNotes.mockReset().mockResolvedValue({ moved: 2, collided: [] });
    h.showNotesFolderInFinder.mockReset().mockResolvedValue(undefined);
    h.showNotesFileInFileManager.mockReset().mockResolvedValue(undefined);
    h.getNotesFolder.mockReset().mockResolvedValue({
      path: "/home/user/Writ",
      display_path: "~/Writ",
      fallback: null,
    });
  });

  it("report_is_fetched_once_on_mount", async () => {
    const store = await freshStore();
    await store.load();
    await store.load();
    expect(h.getNotesMigrationReport).toHaveBeenCalledTimes(1);
    expect(store.report()?.migrated).toBe(3);
  });

  it("dismiss_clears_the_panel", async () => {
    const store = await freshStore();
    await store.load();
    expect(store.report()).not.toBeNull();

    await store.dismiss();

    expect(store.report()).toBeNull();
    expect(h.dismissNotesMigrationReport).toHaveBeenCalledTimes(1);
  });

  it("move_archived_reports_collisions", async () => {
    h.moveArchivedNotes.mockResolvedValue({ moved: 2, collided: ["Meeting.md"] });
    const store = await freshStore();
    await store.load();

    const outcome = await store.moveArchived();

    expect(outcome).toEqual({ moved: 2, collided: ["Meeting.md"] });
    expect(store.report()?.archived).toBe(0);
    expect(store.report()?.migrated).toBe(5);
  });

  it("keeps the offer for a note that would not move", async () => {
    h.getNotesMigrationReport.mockResolvedValue(report({ archived: 3 }));
    h.moveArchivedNotes.mockResolvedValue({ moved: 2, collided: [] });
    const store = await freshStore();
    await store.load();

    await store.moveArchived();

    expect(store.report()?.archived).toBe(1);
  });

  it("shows nothing when the migration left no report", async () => {
    h.getNotesMigrationReport.mockResolvedValue(null);
    const store = await freshStore();
    await store.load();
    expect(store.report()).toBeNull();
  });

  it("asks where the notes are now before opening the recovered folder", async () => {
    const store = await freshStore();
    await store.load();
    await store.showRecovered();
    expect(h.showNotesFileInFileManager).toHaveBeenCalledWith("/home/user/Writ/Recovered");
  });
});
