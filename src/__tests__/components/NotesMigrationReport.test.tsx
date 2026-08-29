import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  report: vi.fn(),
  load: vi.fn(),
  dismiss: vi.fn(),
  showInFileManager: vi.fn(),
  showRecovered: vi.fn(),
  moveArchived: vi.fn(),
  folder: vi.fn(),
  loadFolder: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock("../../stores/global/notes-migration", () => ({
  notesMigrationStore: {
    report: mocks.report,
    load: mocks.load,
    dismiss: mocks.dismiss,
    showInFileManager: mocks.showInFileManager,
    showRecovered: mocks.showRecovered,
    moveArchived: mocks.moveArchived,
  },
}));

vi.mock("../../stores/global/notes", () => ({
  notesStore: { folder: mocks.folder, loadFolder: mocks.loadFolder },
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: mocks.showToast,
  default: () => null,
}));

import NotesMigrationReport from "../../components/NotesMigrationReport/NotesMigrationReport";

function report(overrides: Record<string, unknown> = {}) {
  return {
    ran_at: "2026-08-30T09:00:00Z",
    first_ran_at: "2026-08-30T09:00:00Z",
    notes_folder: "/home/user/Writ",
    archive_folder: "/home/user/.writ/archive",
    migrated: 3,
    archived: 0,
    recovered: 1,
    failed: 0,
    deleted_empty: 0,
    piped: 1,
    ...overrides,
  };
}

function text(container: Element): string {
  return container.querySelector("[data-notes-report]")?.textContent ?? "";
}

describe("NotesMigrationReport", () => {
  beforeEach(() => {
    mocks.report.mockReset().mockReturnValue(report());
    mocks.load.mockReset().mockResolvedValue(undefined);
    mocks.dismiss.mockReset().mockResolvedValue(undefined);
    mocks.showInFileManager.mockReset().mockResolvedValue(undefined);
    mocks.showRecovered.mockReset().mockResolvedValue(undefined);
    mocks.moveArchived.mockReset().mockResolvedValue({ moved: 2, collided: [] });
    mocks.folder
      .mockReset()
      .mockReturnValue({ path: "/home/user/Writ", display_path: "~/Writ", fallback_from: null });
    mocks.loadFolder.mockReset().mockResolvedValue(undefined);
    mocks.showToast.mockReset();
  });

  afterEach(cleanup);

  it("draws nothing when there is no report", () => {
    mocks.report.mockReturnValue(null);
    const { container } = render(() => <NotesMigrationReport />);
    expect(container.querySelector("[data-notes-report]")).toBeNull();
  });

  // migrated + piped + recovered: every note the pass put in the folder.
  it("counts every note that became a file in the folder", () => {
    const { container } = render(() => <NotesMigrationReport />);
    expect(text(container)).toContain("5 notes are now files in ~/Writ.");
    expect(text(container)).not.toContain("archive");
    expect(container.querySelector("[data-action='notes-report-show']")).not.toBeNull();
  });

  it("offers to move the archived notes only when there are some", async () => {
    mocks.report.mockReturnValue(report({ archived: 4 }));
    const { container } = render(() => <NotesMigrationReport />);
    expect(text(container)).toContain("4 older notes are waiting in an archive folder.");

    fireEvent.click(container.querySelector("[data-action='notes-report-archive']")!);
    await waitFor(() => expect(mocks.moveArchived).toHaveBeenCalledTimes(1));
  });

  it("links to the details only when a note could not be checked", async () => {
    mocks.report.mockReturnValue(report({ failed: 2 }));
    const { container } = render(() => <NotesMigrationReport />);
    expect(text(container)).toContain("2 notes could not be checked.");

    fireEvent.click(container.querySelector("[data-action='notes-report-details']")!);
    await waitFor(() => expect(mocks.showRecovered).toHaveBeenCalledTimes(1));
  });

  it("never says a note is still inside Writ", () => {
    mocks.report.mockReturnValue(report({ archived: 4, failed: 2 }));
    const { container } = render(() => <NotesMigrationReport />);
    expect(text(container)).not.toContain("still inside Writ");
  });

  it("dismisses", async () => {
    const { container } = render(() => <NotesMigrationReport />);
    fireEvent.click(container.querySelector("[data-action='notes-report-dismiss']")!);
    await waitFor(() => expect(mocks.dismiss).toHaveBeenCalledTimes(1));
  });
});
