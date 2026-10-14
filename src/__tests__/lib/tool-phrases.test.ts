import { describe, it, expect } from "vitest";

import { describeCall, isReadAction } from "../../lib/tool-phrases";

describe("describeCall", () => {
  it("names the file after a call that names one", () => {
    expect(describeCall("read_note", true, "allow")).toBe("asked to open");
    expect(describeCall("write_note", true, "allow")).toBe("replaced the text of");
  });

  it("reads every read as asked for, because its line is written before it runs", () => {
    expect(describeCall("read_note", true, "allow")).toBe("asked to open");
    expect(describeCall("search_notes", false, "allow")).toBe("asked to search files");
    expect(describeCall("note_links", true, "allow")).toBe("asked to read the links in");
    expect(describeCall("folder_tags", false, "allow")).toBe("asked to list the tags in your folder");
  });

  it("reads a call that did not go through as asked for, not as done", () => {
    expect(describeCall("write_note", true, "pending")).toBe("asked to replace the text of");
    expect(describeCall("read_note", true, "refuse")).toBe("asked to open");
    expect(describeCall("list_notes", false, "refuse")).toBe("asked to list files");
    expect(describeCall("discard_proposal", true, "refuse")).toBe("asked to change");
  });

  it("says what was asked for without a file when the line names none", () => {
    expect(describeCall("read_note", false, "allow")).toBe("asked to open a file");
    expect(describeCall("list_notes", false, "allow")).toBe("asked to list files");
    expect(describeCall("create_note", false, "allow")).toBe("made a new file");
  });

  it("reads an allowed rename as the file's new name", () => {
    expect(describeCall("rename_note", true, "allow")).toBe("renamed a file to");
    expect(describeCall("rename_note", true, "refuse")).toBe("asked to rename");
  });

  it("spells an action it does not know as words", () => {
    expect(describeCall("merge_notes", true, "allow")).toBe("merge notes");
  });
});

describe("isReadAction", () => {
  it("is true for read tools only", () => {
    expect(isReadAction("read_note")).toBe(true);
    expect(isReadAction("folder_tags")).toBe(true);
    expect(isReadAction("write_note")).toBe(false);
    expect(isReadAction("apply_proposal")).toBe(false);
    expect(isReadAction("undo_proposal")).toBe(false);
  });

  it("names a chat Undo in words", () => {
    expect(describeCall("undo_proposal", true, "allow")).toBe("put back an earlier version of");
    expect(describeCall("undo_proposal", false, "refuse")).toBe("asked to put back an earlier version");
  });
});
