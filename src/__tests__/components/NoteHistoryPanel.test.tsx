import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

import type { NoteVersion } from "../../services/tauri";

// The panel over the real store, with only the IPC layer replaced: what the
// two controls do has to reach a service, and the store is the thing that
// takes them there.

const h = vi.hoisted(() => ({
  noteVersions: vi.fn(),
  noteVersionContent: vi.fn(),
  restoreNoteVersion: vi.fn(),
  copyNoteVersion: vi.fn(),
}));

vi.mock("../../services/tauri", () => ({
  noteVersions: h.noteVersions,
  noteVersionContent: h.noteVersionContent,
  restoreNoteVersion: h.restoreNoteVersion,
  copyNoteVersion: h.copyNoteVersion,
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

function given(versions: NoteVersion[], text = "what the note said\n") {
  h.noteVersions.mockResolvedValue(versions);
  h.noteVersionContent.mockResolvedValue(text);
  h.restoreNoteVersion.mockResolvedValue({ note: "Launch.md", bytes: 1_200 });
  h.copyNoteVersion.mockResolvedValue({ name: "Launch (recovered 2026-09-10 14.32.05).md" });
}

afterEach(() => {
  closeNoteVersions();
  h.noteVersions.mockReset();
  h.noteVersionContent.mockReset();
  h.restoreNoteVersion.mockReset();
  h.copyNoteVersion.mockReset();
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

  it("copies the selected version once", async () => {
    given([version(11, at(0, 14, 32)), version(12, at(2, 9, 11))]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("Copy this version")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: "Copy this version" }));

    await waitFor(() => expect(h.copyNoteVersion).toHaveBeenCalledTimes(1));
    expect(h.copyNoteVersion).toHaveBeenCalledWith(11);
  });

  it("copies the version a row selects rather than the newest", async () => {
    given([version(11, at(0, 14, 32)), version(12, at(2, 9, 11))]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);
    await waitFor(() => expect(screen.getByText("Copy this version")).toBeTruthy());

    fireEvent.click(screen.getByText(dateOf(at(2, 9, 11))));
    await waitFor(() => expect(h.noteVersionContent).toHaveBeenCalledWith(12));
    fireEvent.click(screen.getByRole("button", { name: "Restore this version" }));

    await waitFor(() => expect(h.restoreNoteVersion).toHaveBeenCalledWith(12));
  });

  it("offers the note nothing to restore when nothing is kept for it", async () => {
    given([]);

    const screen = render(() => <NoteHistoryPanel />);
    openNoteVersions(NOTE);

    await waitFor(() => expect(screen.getByText("No versions of this note yet.")).toBeTruthy());
    expect(screen.queryByText("Restore this version")).toBeNull();
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
  });
});
