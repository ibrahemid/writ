import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  resolveNoteLink: vi.fn(),
  noteNameCandidates: vi.fn(),
  newNamedNote: vi.fn(),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import { linkStore, type LinkResolution } from "../../stores/global/link";
import * as api from "../../services/tauri";
import * as events from "../../services/events";
import { showToast } from "../../components/Notifications/Toast";

const mockedApi = vi.mocked(api);
const mockedEvents = vi.mocked(events);

const FROM = "/notes/From.md";

function resolved(path: string): LinkResolution {
  return { status: "resolved", path, candidates: [], heading_line: null };
}

/** The handler the store gave `onEvent`, so a test can deliver the event. */
function notesChangedHandler(): () => void {
  const call = mockedEvents.onEvent.mock.calls.find(([kind]) => kind === "notes:changed");
  expect(call, "the store never subscribed to notes:changed").toBeDefined();
  return call![1] as () => void;
}

beforeEach(async () => {
  await linkStore.reset();
  vi.clearAllMocks();
  mockedEvents.onEvent.mockResolvedValue(() => {});
});

describe("linkStore resolution cache", () => {
  it("reads a target once and serves it from what it holds", async () => {
    mockedApi.resolveNoteLink.mockResolvedValue(resolved("/notes/Target.md"));

    expect(await linkStore.resolveNoteLink(FROM, "Target")).toEqual(
      resolved("/notes/Target.md"),
    );
    expect(await linkStore.resolveNoteLink(FROM, "Target")).toEqual(
      resolved("/notes/Target.md"),
    );
    expect(mockedApi.resolveNoteLink).toHaveBeenCalledTimes(1);
    expect(linkStore.knownNoteLink(FROM, "Target")?.status).toBe("resolved");
  });

  it("keys the same target in two notes apart", async () => {
    mockedApi.resolveNoteLink
      .mockResolvedValueOnce(resolved("/notes/a/Target.md"))
      .mockResolvedValueOnce(resolved("/notes/b/Target.md"));

    await linkStore.resolveNoteLink("/notes/a/One.md", "Target");
    await linkStore.resolveNoteLink("/notes/b/Two.md", "Target");
    expect(mockedApi.resolveNoteLink).toHaveBeenCalledTimes(2);
    expect(linkStore.knownNoteLink("/notes/a/One.md", "Target")?.path).toBe(
      "/notes/a/Target.md",
    );
  });

  it("asks once for two reads that overlap", async () => {
    mockedApi.resolveNoteLink.mockResolvedValue(resolved("/notes/Target.md"));
    const both = await Promise.all([
      linkStore.resolveNoteLink(FROM, "Target"),
      linkStore.resolveNoteLink(FROM, "Target"),
    ]);
    expect(both[0]).toEqual(both[1]);
    expect(mockedApi.resolveNoteLink).toHaveBeenCalledTimes(1);
  });

  // A note added or removed changes what a target names, so nothing that was
  // read before it stays authoritative.
  it("drops what it holds when a note changes on disk", async () => {
    mockedApi.resolveNoteLink.mockResolvedValue(resolved("/notes/Target.md"));
    await linkStore.resolveNoteLink(FROM, "Target");

    notesChangedHandler()();
    expect(linkStore.knownNoteLink(FROM, "Target")).toBeNull();

    await linkStore.resolveNoteLink(FROM, "Target");
    expect(mockedApi.resolveNoteLink).toHaveBeenCalledTimes(2);
  });

  // What the editor reads to know its record of "already asked" is stale.
  it("says so through the generation every time it drops what it holds", async () => {
    mockedApi.resolveNoteLink.mockResolvedValue(resolved("/notes/Target.md"));
    await linkStore.resolveNoteLink(FROM, "Target");
    const before = linkStore.resolutionGeneration();

    notesChangedHandler()();
    expect(linkStore.resolutionGeneration()).toBe(before + 1);

    mockedApi.newNamedNote.mockResolvedValue({ id: "b1", source_path: "/n/New.md" } as never);
    await linkStore.createNote("New");
    expect(linkStore.resolutionGeneration()).toBe(before + 2);
  });

  it("reports a failed read as missing and does not hold it", async () => {
    mockedApi.resolveNoteLink.mockRejectedValue("the index could not be read");
    expect((await linkStore.resolveNoteLink(FROM, "Target")).status).toBe("missing");
    expect(linkStore.knownNoteLink(FROM, "Target")).toBeNull();
  });
});

describe("linkStore createNote", () => {
  it("creates the note and drops what the cache held", async () => {
    mockedApi.resolveNoteLink.mockResolvedValue({
      status: "missing",
      path: null,
      candidates: [],
      heading_line: null,
    });
    await linkStore.resolveNoteLink(FROM, "New");
    mockedApi.newNamedNote.mockResolvedValue({
      id: "b1",
      source_path: "/notes/New.md",
    } as never);

    const doc = await linkStore.createNote("New");
    expect(mockedApi.newNamedNote).toHaveBeenCalledWith("New");
    expect(doc?.source_path).toBe("/notes/New.md");
    expect(linkStore.knownNoteLink(FROM, "New")).toBeNull();
  });

  it("reports a refusal rather than swallowing it", async () => {
    mockedApi.newNamedNote.mockRejectedValue("A note named \"New.md\" is already there.");
    expect(await linkStore.createNote("New")).toBeNull();
    expect(showToast).toHaveBeenCalledWith(
      "A note named \"New.md\" is already there.",
      "error",
    );
  });
});

describe("linkStore noteNameCandidates", () => {
  it("answers with the hits the index ranked", async () => {
    mockedApi.noteNameCandidates.mockResolvedValue([
      { path: "/notes/Grocery list.md", name: "Grocery list" },
    ]);
    expect(await linkStore.noteNameCandidates("Gro")).toEqual([
      { path: "/notes/Grocery list.md", name: "Grocery list" },
    ]);
  });

  it("answers with nothing when the index cannot be read", async () => {
    mockedApi.noteNameCandidates.mockRejectedValue("no index");
    expect(await linkStore.noteNameCandidates("Gro")).toEqual([]);
  });
});
