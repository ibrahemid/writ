import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../services/tauri", () => ({
  resolveNoteLink: vi.fn(),
  noteNameCandidates: vi.fn(),
  newNoteFromLink: vi.fn(),
  noteHeadingLine: vi.fn(),
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

    mockedApi.newNoteFromLink.mockResolvedValue({ id: "b1", source_path: "/n/New.md" } as never);
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
    mockedApi.newNoteFromLink.mockResolvedValue({
      id: "b1",
      source_path: "/notes/New.md",
    } as never);

    const doc = await linkStore.createNote("New");
    expect(mockedApi.newNoteFromLink).toHaveBeenCalledWith("New");
    // The target goes to Rust as written: it decides the folder and the name.
    await linkStore.createNote("projects/Ideas.md");
    expect(mockedApi.newNoteFromLink).toHaveBeenCalledWith("projects/Ideas.md");
    expect(doc?.source_path).toBe("/notes/New.md");
    expect(linkStore.knownNoteLink(FROM, "New")).toBeNull();
  });

  it("reports a refusal rather than swallowing it", async () => {
    mockedApi.newNoteFromLink.mockRejectedValue("A note named \"New.md\" is already there.");
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

// A `[[…]]` the preview resolved is written with a scheme of its own, because
// a note is not a web address and the external-link policy refuses one as
// unparseable. The scheme says what the frame wants; the notes folder decides
// what it gets.
describe("linkStore notePathFromPreview", () => {
  const ROOT = "/Users/x/Writ";
  const FROM_NOTE = "/Users/x/Writ/From.md";

  /** The index knows one note, whatever target it is asked about. */
  function indexKnows(path: string | null) {
    mockedApi.resolveNoteLink.mockImplementation(async () =>
      path === null
        ? { status: "missing", path: null, candidates: [], heading_line: null }
        : resolved(path),
    );
  }

  it("answers with the note the preview link names", async () => {
    indexKnows("/Users/x/Writ/Note.md");
    expect(await linkStore.notePathFromPreview("writ-note:Note.md", ROOT, FROM_NOTE)).toBe(
      "/Users/x/Writ/Note.md",
    );
    indexKnows("/Users/x/Writ/folder/Deep.md");
    expect(
      await linkStore.notePathFromPreview("writ-note:folder/Deep.md", ROOT, FROM_NOTE),
    ).toBe("/Users/x/Writ/folder/Deep.md");
  });

  // The fragment names a heading in the note, not part of its file name.
  it("opens the note a link to a heading names", async () => {
    indexKnows("/Users/x/Writ/Note.md");
    expect(
      await linkStore.notePathFromPreview("writ-note:Note.md#some-heading", ROOT, FROM_NOTE),
    ).toBe("/Users/x/Writ/Note.md");
    expect(mockedApi.resolveNoteLink).toHaveBeenCalledWith(FROM_NOTE, "Note.md");
  });

  it("decodes a name written with a character the href escaped", async () => {
    indexKnows("/Users/x/Writ/a#b.md");
    expect(await linkStore.notePathFromPreview("writ-note:a%23b.md", ROOT, FROM_NOTE)).toBe(
      "/Users/x/Writ/a#b.md",
    );
    expect(mockedApi.resolveNoteLink).toHaveBeenCalledWith(FROM_NOTE, "a#b.md");
  });

  // Anything in the rendered document can post an href, so the scheme is a
  // claim and not a permission.
  it("refuses a path that lands outside the notes folder", async () => {
    indexKnows("/Users/x/Writ/Note.md");
    for (const href of [
      "writ-note:../../.ssh/id_rsa",
      "writ-note:/etc/passwd",
      "writ-note:folder/../../secrets.md",
      "writ-note:%2e%2e/%2e%2e/passwd",
      "writ-note:..%2f..%2fsecrets.md",
      "writ-note:",
      "writ-note:#some-heading",
    ]) {
      expect(await linkStore.notePathFromPreview(href, ROOT, FROM_NOTE), href).toBeNull();
    }
  });

  // Raw HTML in a note can write the scheme, and the renderer only writes it
  // for a target the index resolved. A file inside the folder that names no
  // note is confirmed the way every other link is.
  it("refuses a path inside the folder that names no note the index holds", async () => {
    indexKnows(null);
    expect(
      await linkStore.notePathFromPreview("writ-note:.obsidian/workspace.json", ROOT, FROM_NOTE),
    ).toBeNull();
    expect(await linkStore.notePathFromPreview("writ-note:Note.md", ROOT, FROM_NOTE)).toBeNull();
  });

  it("leaves every other href to the external-link policy", async () => {
    indexKnows("/Users/x/Writ/Note.md");
    expect(await linkStore.notePathFromPreview("https://example.com/x", ROOT, FROM_NOTE)).toBeNull();
    expect(await linkStore.notePathFromPreview("Note.md", ROOT, FROM_NOTE)).toBeNull();
    expect(await linkStore.notePathFromPreview("javascript:alert(1)", ROOT, FROM_NOTE)).toBeNull();
    expect(mockedApi.resolveNoteLink).not.toHaveBeenCalled();
  });

  it("answers with nothing when there is no notes folder", async () => {
    indexKnows("/Users/x/Writ/Note.md");
    expect(await linkStore.notePathFromPreview("writ-note:Note.md", null, FROM_NOTE)).toBeNull();
    expect(await linkStore.notePathFromPreview("writ-note:Note.md", "", FROM_NOTE)).toBeNull();
  });

  // A preview of something with no file has nothing to resolve against, and a
  // guess would be a file opened from a claim nothing checked.
  it("answers with nothing when the previewed buffer has no file", async () => {
    indexKnows("/Users/x/Writ/Note.md");
    expect(await linkStore.notePathFromPreview("writ-note:Note.md", ROOT, null)).toBeNull();
    expect(await linkStore.notePathFromPreview("writ-note:Note.md", ROOT, "")).toBeNull();
    expect(mockedApi.resolveNoteLink).not.toHaveBeenCalled();
  });
});

// The editor lands on the heading a `[[Note#Section]]` names, and the preview
// carries that heading as the href's fragment.
describe("linkStore noteOpenFromPreview", () => {
  const ROOT = "/Users/x/Writ";
  const FROM_NOTE = "/Users/x/Writ/From.md";
  const NOTE = "/Users/x/Writ/Note.md";

  beforeEach(() => {
    mockedApi.resolveNoteLink.mockImplementation(async () => resolved(NOTE));
  });

  it("answers with the line the heading sits on", async () => {
    mockedApi.noteHeadingLine.mockResolvedValue(12);
    expect(await linkStore.noteOpenFromPreview("writ-note:Note.md#later-part", ROOT, FROM_NOTE))
      .toEqual({ path: NOTE, headingLine: 12 });
    expect(mockedApi.noteHeadingLine).toHaveBeenCalledWith(NOTE, "later-part");
  });

  it("unescapes an anchor the href escaped", async () => {
    mockedApi.noteHeadingLine.mockResolvedValue(3);
    await linkStore.noteOpenFromPreview("writ-note:Note.md#caf%C3%A9", ROOT, FROM_NOTE);
    expect(mockedApi.noteHeadingLine).toHaveBeenCalledWith(NOTE, "café");
  });

  it("opens at the top when the href names no heading", async () => {
    expect(await linkStore.noteOpenFromPreview("writ-note:Note.md", ROOT, FROM_NOTE)).toEqual({
      path: NOTE,
      headingLine: null,
    });
    expect(mockedApi.noteHeadingLine).not.toHaveBeenCalled();
  });

  it("opens at the top when the note has no such heading", async () => {
    mockedApi.noteHeadingLine.mockResolvedValue(null);
    expect(await linkStore.noteOpenFromPreview("writ-note:Note.md#gone", ROOT, FROM_NOTE)).toEqual({
      path: NOTE,
      headingLine: null,
    });
  });

  it("opens at the top when the line cannot be read", async () => {
    mockedApi.noteHeadingLine.mockRejectedValue("no index");
    expect(await linkStore.noteOpenFromPreview("writ-note:Note.md#h", ROOT, FROM_NOTE)).toEqual({
      path: NOTE,
      headingLine: null,
    });
  });

  // The gate is the one every forged href already meets; a fragment does not
  // widen it and nothing is asked about a href that opens nothing.
  it("answers with nothing for a href that opens no note", async () => {
    for (const href of [
      "writ-note:../../.ssh/id_rsa#h",
      "https://example.com/x#h",
      "writ-note:#h",
    ]) {
      expect(await linkStore.noteOpenFromPreview(href, ROOT, FROM_NOTE), href).toBeNull();
    }
    expect(mockedApi.noteHeadingLine).not.toHaveBeenCalled();
  });
});
