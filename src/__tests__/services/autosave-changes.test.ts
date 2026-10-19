import { describe, it, expect, vi, beforeEach } from "vitest";
import { ChangeSet, EditorState, Text } from "@codemirror/state";

vi.mock("../../services/tauri", () => ({
  saveBufferContent: vi.fn(),
  saveBufferChanges: vi.fn(),
  recordUnsavedNotes: vi.fn().mockResolvedValue(undefined),
  discardUnsavedText: vi.fn().mockResolvedValue(undefined),
}));

import {
  forgetLandedText,
  onAutosaveSuccess,
  peekUnsavedContent,
  recordOpenedText,
  resetAutosave,
  saveNow,
} from "../../services/autosave";
import { encodeChangesSince } from "../../editor/change-diff";
import { saveBufferChanges, saveBufferContent } from "../../services/tauri";

const mockedWhole = vi.mocked(saveBufferContent);
const mockedChanges = vi.mocked(saveBufferChanges);

// The large tiers save the changes since the last write that landed (EP-13).
// What Rust would write for them is the base with the changes applied, which
// this fake keeps per note so a test can read what the file holds.
const files = new Map<string, string>();
let digests = 0;
const digestOf = new Map<string, string>();

function digest(text: string): string {
  const known = [...digestOf.entries()].find(([, value]) => value === text);
  if (known) return known[0];
  const name = `d${++digests}`;
  digestOf.set(name, text);
  return name;
}

function lines(count: number): Text {
  return Text.of(Array.from({ length: count }, (_, n) => `line ${n}`));
}

function edit(doc: Text, from: number, insert: string): Text {
  return EditorState.create({ doc }).update({ changes: { from, insert } }).state.doc;
}

describe("autosave: large-tier saves send their changes", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetAutosave();
    files.clear();
    digestOf.clear();
    mockedWhole.mockImplementation(async (id: string, content: string) => {
      files.set(id, content);
      return digest(content);
    });
    mockedChanges.mockImplementation(async (id, base, changes, length) => {
      const text = files.get(id) ?? "";
      if (digestOf.get(base) !== text) throw "ERR_CHANGES_STALE: the file moved on";
      const doc = ChangeSet.fromJSON(changes).apply(Text.of(text.split("\n")));
      expect(doc.length).toBe(length);
      files.set(id, doc.toString());
      return digest(doc.toString());
    });
  });

  it("sends the first write whole and the next as the changes since it", async () => {
    const opened = lines(50_000);
    expect((await saveNow("big", opened, undefined, encodeChangesSince)).ok).toBe(true);
    expect(mockedWhole).toHaveBeenCalledTimes(1);

    const typed = edit(opened, 120, "typed");
    const materialised = vi.spyOn(typed, "toString");
    expect((await saveNow("big", typed, undefined, encodeChangesSince)).ok).toBe(true);

    expect(mockedWhole).toHaveBeenCalledTimes(1);
    expect(mockedChanges).toHaveBeenCalledTimes(1);
    const [, base, changes] = mockedChanges.mock.calls[0];
    expect(base).toBe(digest(opened.toString()));
    expect(JSON.stringify(changes).length).toBeLessThan(100);
    expect(materialised).not.toHaveBeenCalled();
    expect(files.get("big")).toBe(typed.toString());
  });

  it("tells the success listener about one write, as the snapshot it carried", async () => {
    const heard: unknown[] = [];
    const stop = onAutosaveSuccess((_id, _hash, _writeBack, write) => heard.push(write.source));
    const opened = lines(1_000);
    await saveNow("big", opened, undefined, encodeChangesSince);
    const typed = edit(opened, 0, "x");
    await saveNow("big", typed, undefined, encodeChangesSince);
    stop();
    expect(heard).toEqual([opened, typed]);
  });

  it("sends the whole text again when the file moved on from the base", async () => {
    const opened = lines(1_000);
    await saveNow("big", opened, undefined, encodeChangesSince);
    files.set("big", "another program wrote this");

    const typed = edit(opened, 0, "x");
    expect((await saveNow("big", typed, undefined, encodeChangesSince)).ok).toBe(true);

    expect(mockedChanges).toHaveBeenCalledTimes(1);
    expect(mockedWhole).toHaveBeenCalledTimes(2);
    expect(mockedWhole.mock.calls[1][1]).toBe(typed.toString());
  });

  it("keeps the text for the next attempt when the change set's write fails", async () => {
    const opened = lines(1_000);
    await saveNow("big", opened, undefined, encodeChangesSince);
    mockedChanges.mockRejectedValueOnce("ERR_WRITE_FAILED: disk full");

    const typed = edit(opened, 0, "x");
    expect((await saveNow("big", typed, undefined, encodeChangesSince)).ok).toBe(false);

    expect(peekUnsavedContent("big")).toBe(typed.toString());
  });

  it("sends the whole text for a note in the normal tier", async () => {
    const opened = lines(100);
    await saveNow("small", opened);
    await saveNow("small", edit(opened, 0, "x"));
    expect(mockedChanges).not.toHaveBeenCalled();
    expect(mockedWhole).toHaveBeenCalledTimes(2);
  });

  it("sends the whole text after the landed document is let go", async () => {
    const opened = lines(1_000);
    await saveNow("big", opened, undefined, encodeChangesSince);
    forgetLandedText("big");
    await saveNow("big", edit(opened, 0, "x"), undefined, encodeChangesSince);
    expect(mockedChanges).not.toHaveBeenCalled();
  });

  it("sends the whole text after a write that landed as a string", async () => {
    const opened = lines(1_000);
    await saveNow("big", opened, undefined, encodeChangesSince);
    await saveNow("big", "a string, not a document", undefined, encodeChangesSince);
    await saveNow("big", edit(opened, 0, "x"), undefined, encodeChangesSince);
    expect(mockedChanges).not.toHaveBeenCalled();
    expect(mockedWhole).toHaveBeenCalledTimes(3);
  });
});

describe("autosave: the first save after the open sends its changes", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetAutosave();
    files.clear();
    digestOf.clear();
    mockedWhole.mockImplementation(async (id: string, content: string) => {
      files.set(id, content);
      return digest(content);
    });
    mockedChanges.mockImplementation(async (id, base, changes, length) => {
      const text = files.get(id) ?? "";
      if (digestOf.get(base) !== text) throw "ERR_CHANGES_STALE: the file moved on";
      const doc = ChangeSet.fromJSON(changes).apply(Text.of(text.split("\n")));
      expect(doc.length).toBe(length);
      files.set(id, doc.toString());
      return digest(doc.toString());
    });
  });

  // What the open leaves: the file holds the document the load read, and
  // Rust answered the open with that text's digest.
  function open(id: string, doc: Text): void {
    files.set(id, doc.toString());
    recordOpenedText(id, doc, digest(doc.toString()));
  }

  it("sends the changes since the opened document, and nothing whole", async () => {
    const opened = lines(50_000);
    open("big", opened);

    const typed = edit(opened, 120, "typed");
    const materialised = vi.spyOn(typed, "toString");
    expect((await saveNow("big", typed, undefined, encodeChangesSince)).ok).toBe(true);

    expect(mockedWhole).not.toHaveBeenCalled();
    expect(mockedChanges).toHaveBeenCalledTimes(1);
    expect(mockedChanges.mock.calls[0][1]).toBe(digest(opened.toString()));
    expect(JSON.stringify(mockedChanges.mock.calls[0][2]).length).toBeLessThan(100);
    expect(materialised).not.toHaveBeenCalled();
    expect(files.get("big")).toBe(typed.toString());
  });

  it("sends the whole text when the file changed after the open", async () => {
    const opened = lines(1_000);
    open("big", opened);
    files.set("big", "another program wrote this");

    const typed = edit(opened, 0, "x");
    expect((await saveNow("big", typed, undefined, encodeChangesSince)).ok).toBe(true);

    expect(mockedChanges).toHaveBeenCalledTimes(1);
    expect(mockedWhole).toHaveBeenCalledTimes(1);
    expect(mockedWhole.mock.calls[0][1]).toBe(typed.toString());
  });

  it("takes no opened document while a write of the note is out", async () => {
    const written = lines(1_000);
    let land!: () => void;
    // The write reaches the file and its answer is a failure, as one whose
    // record could not be updated afterwards is.
    mockedWhole.mockImplementationOnce(async (id: string, content: string) => {
      await new Promise<void>((resolve) => (land = resolve));
      files.set(id, content);
      throw "ERR_WRITE_FAILED: the record was not updated";
    });
    const out = saveNow("big", written, undefined, encodeChangesSince);
    await Promise.resolve();

    // The open's answer arrives while the write is out, and its digest
    // already describes what the write put on the file.
    recordOpenedText("big", lines(999), digest(written.toString()));
    land();
    expect((await out).ok).toBe(false);

    const typed = edit(written, 0, "x");
    expect((await saveNow("big", typed, undefined, encodeChangesSince)).ok).toBe(true);
    expect(mockedChanges).not.toHaveBeenCalled();
    expect(files.get("big")).toBe(typed.toString());
  });
});
