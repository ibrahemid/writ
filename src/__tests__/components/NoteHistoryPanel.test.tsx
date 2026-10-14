import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

import type { NoteVersion, NoVersionsReason } from "../../services/tauri";

// The panel over the real store, with only the IPC layer replaced: what the
// two controls do has to reach a service, and the store is the thing that
// takes them there.

const h = vi.hoisted(() => ({
  noteVersions: vi.fn(),
  noteVersionContent: vi.fn(),
  restoreNoteVersion: vi.fn(),
  copyNoteVersion: vi.fn(),
  compareNoteVersion: vi.fn(),
  focusEditor: vi.fn(),
}));

vi.mock("../../components/WindowProvider/WindowProvider", () => ({
  useWindow: () => ({ editor: { focusEditor: h.focusEditor } }),
}));

vi.mock("../../services/tauri", () => ({
  noteVersions: h.noteVersions,
  noteVersionContent: h.noteVersionContent,
  restoreNoteVersion: h.restoreNoteVersion,
  copyNoteVersion: h.copyNoteVersion,
  compareNoteVersion: h.compareNoteVersion,
}));

import NoteHistoryPanel, {
  openNoteVersions,
  closeNoteVersions,
} from "../../components/NoteHistory/NoteHistoryPanel";

const NOTE = "/notes/Launch.md";

/** Days back from now, on the hour so the label is stable. */
function at(daysAgo: number, hour: number, minute: number): number {
  const stamp = new Date();
  stamp.setDate(stamp.getDate() - daysAgo);
  stamp.setHours(hour, minute, 0, 0);
  return stamp.getTime();
}

function version(id: number, atMs: number, bytes = 1_200): NoteVersion {
  return { id, at_ms: atMs, bytes };
}

function timeOf(atMs: number): string {
  return new Date(atMs).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function dateOf(atMs: number): string {
  return new Date(atMs).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function given(
  versions: NoteVersion[],
  text = "what the note said\n",
  reason: NoVersionsReason | null = versions.length > 0 ? null : "none_yet",
) {
  h.noteVersions.mockResolvedValue({ versions, no_versions_reason: reason });
  h.noteVersionContent.mockResolvedValue(text);
  h.restoreNoteVersion.mockResolvedValue({ note: "Launch.md", bytes: 1_200, replaced: 42 });
  h.copyNoteVersion.mockResolvedValue({ name: "Launch (recovered 2026-09-10 14.32.05).md" });
}

afterEach(() => {
  closeNoteVersions();
  h.noteVersions.mockReset();
  h.noteVersionContent.mockReset();
  h.restoreNoteVersion.mockReset();
  h.copyNoteVersion.mockReset();
  h.compareNoteVersion.mockReset();
  cleanup();
});

describe("NoteHistoryPanel", () => {
  it("names today's versions by the time and older ones by the date", async () => {
    const today = at(0, 14, 32);
    const yesterday = at(1, 9, 11);
    const older = at(6, 16, 4);
    given([version(1, today), version(2, yesterday), version(3, older)]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);

    await waitFor(() => expect(screen.getByText(`Today ${timeOf(today)}`)).toBeTruthy());
    expect(screen.getByText(`Yesterday ${timeOf(yesterday)}`)).toBeTruthy();
    expect(screen.getByText(dateOf(older))).toBeTruthy();
  });

  it("reads the newest version as soon as it opens", async () => {
    given([version(7, at(0, 14, 32))], "the newest text\n");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);

    await waitFor(() => expect(screen.getByText("the newest text")).toBeTruthy());
    expect(h.noteVersions).toHaveBeenCalledTimes(1);
    expect(h.noteVersions).toHaveBeenCalledWith(NOTE);
    expect(h.noteVersionContent).toHaveBeenCalledTimes(1);
    expect(h.noteVersionContent).toHaveBeenCalledWith(7);
  });

  it("restores the selected version once", async () => {
    given([version(11, at(0, 14, 32)), version(12, at(2, 9, 11))]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("Restore this version")).toBeTruthy());

    fireEvent.click(screen.getByText("Restore this version"));

    await waitFor(() => expect(h.restoreNoteVersion).toHaveBeenCalledTimes(1));
    expect(h.restoreNoteVersion).toHaveBeenCalledWith(11);
  });

  it("saves the selected version as a new file once", async () => {
    given([version(11, at(0, 14, 32)), version(12, at(2, 9, 11))]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("Save as a new file")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Save as a new file" }));

    await waitFor(() => expect(h.copyNoteVersion).toHaveBeenCalledTimes(1));
    expect(h.copyNoteVersion).toHaveBeenCalledWith(11);
  });

  it("copies the version a row selects rather than the newest", async () => {
    given([version(11, at(0, 14, 32)), version(12, at(2, 9, 11))]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("Save as a new file")).toBeTruthy());

    fireEvent.click(screen.getByText(dateOf(at(2, 9, 11))));
    await waitFor(() => expect(h.noteVersionContent).toHaveBeenCalledWith(12));
    fireEvent.click(screen.getByRole("button", { name: "Restore this version" }));

    await waitFor(() => expect(h.restoreNoteVersion).toHaveBeenCalledWith(12));
  });

  it("offers the note nothing to restore when nothing is kept for it", async () => {
    given([]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);

    await waitFor(() => expect(screen.getByText("No versions of this file yet.")).toBeTruthy());
    expect(screen.queryByText("Restore this version")).toBeNull();
  });

  it("says a file over 2 MB is not versioned instead of promising a version", async () => {
    given([], "", "too_large");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions("/logs/server.log");

    await waitFor(() =>
      expect(screen.getByText("Files over 2 MB are not versioned.")).toBeTruthy(),
    );
    expect(screen.queryByText("Writ keeps one each time the file is saved.")).toBeNull();
  });

  it("says a file outside the folder has no versions", async () => {
    given([], "", "outside_folder");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions("/elsewhere/config.toml");

    await waitFor(() =>
      expect(screen.getByText("Only files in your folder have versions.")).toBeTruthy(),
    );
  });

  it("says the versions could not be read, never that there are none, and tries again", async () => {
    h.noteVersions.mockRejectedValueOnce("the version store is locked");
    h.noteVersions.mockResolvedValueOnce({
      versions: [version(11, at(0, 14, 32))],
      no_versions_reason: null,
    });
    h.noteVersionContent.mockResolvedValue("what the note said\n");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);

    await waitFor(() =>
      expect(screen.getByText("Could not read the versions of this file.")).toBeTruthy(),
    );
    expect(screen.queryByText("No versions of this file yet.")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Try again" }));

    await waitFor(() => expect(screen.getByText("Restore this version")).toBeTruthy());
    expect(h.noteVersions).toHaveBeenCalledTimes(2);
  });

  it("takes a restore back from the panel", async () => {
    given([version(11, at(0, 14, 32)), version(12, at(2, 9, 11))]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("Restore this version")).toBeTruthy());

    fireEvent.click(screen.getByText(dateOf(at(2, 9, 11))));
    await waitFor(() => expect(h.noteVersionContent).toHaveBeenCalledWith(12));
    fireEvent.click(screen.getByRole("button", { name: "Restore this version" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Undo" })).toBeTruthy());
    expect(screen.getByText("Restored Launch.md")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Undo" }));

    await waitFor(() => expect(h.restoreNoteVersion).toHaveBeenCalledTimes(2));
    expect(h.restoreNoteVersion).toHaveBeenLastCalledWith(42);
  });

  it("compares the selected version with the current text", async () => {
    given([version(11, at(0, 14, 32))]);
    h.compareNoteVersion.mockResolvedValue([
      {
        before_start: 1,
        after_start: 1,
        lines: [
          { kind: "context", text: "title" },
          { kind: "removed", text: "what it says now" },
          { kind: "added", text: "what it said then" },
        ],
      },
    ]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("Compare with current text")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Compare with current text" }));

    await waitFor(() => expect(screen.getByText("what it said then")).toBeTruthy());
    expect(h.compareNoteVersion).toHaveBeenCalledWith(11);
    const kinds = [...screen.container.querySelectorAll(".note-versions-diff-row")].map((row) =>
      row.getAttribute("data-kind"),
    );
    expect(kinds).toEqual(["context", "removed", "added"]);
  });

  it("says a damaged version is damaged and offers nothing to restore from it", async () => {
    given([version(11, at(0, 14, 32))]);
    h.noteVersionContent.mockRejectedValue("That version is damaged and cannot be restored.");

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);

    await waitFor(() =>
      expect(screen.getByText("That version is damaged and cannot be restored.")).toBeTruthy(),
    );
    const restore = screen.getByRole("button", { name: "Restore this version" });
    expect((restore as HTMLButtonElement).disabled).toBe(true);
  });

  it("says none of the words this feature is not called", async () => {
    given([version(1, at(0, 14, 32)), version(2, at(3, 9, 11))]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("Restore this version")).toBeTruthy());

    const rendered = screen.container.textContent ?? "";
    for (const word of [
      "snapshot",
      "history buffer",
      "restore point",
      "vault",
      "scratchpad",
      "second brain",
    ]) {
      expect(rendered.toLowerCase()).not.toContain(word);
    }
    // One name for the feature, the one Settings and the menu's Revert To… lead to.
    expect(screen.container.querySelector(".note-versions-title")!.textContent!.trim()).toBe(
      "Versions",
    );
    expect(screen.getByRole("listbox").getAttribute("aria-label")).toBe("Versions");
  });
});
