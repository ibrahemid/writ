import { describe, it, expect } from "vitest";

import { describeCall, describeContextTools, isReadAction } from "../../lib/tool-phrases";

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

describe("the tools that change part of a file or show one", () => {
  it("names each in words", () => {
    expect(describeCall("append_note", true, "allow")).toBe("added to");
    expect(describeCall("append_note", false, "refuse")).toBe("asked to add to a file");
    expect(describeCall("replace_in_note", false, "allow")).toBe("replaced a passage in a file");
    expect(describeCall("today", false, "pending")).toBe("asked to use today's file");
    expect(describeCall("open_in_writ", true, "allow")).toBe("asked to show");
    expect(describeCall("read_index", false, "allow")).toBe("read the index of your folder");
  });

  it("counts showing a file as a read", () => {
    expect(isReadAction("open_in_writ")).toBe(true);
    expect(isReadAction("append_note")).toBe(false);
    expect(isReadAction("today")).toBe(false);
  });
});

describe("seeing the open file", () => {
  it("names each read of the open file as done, with the file it named", () => {
    expect(describeCall("get_context", true, "allow")).toBe("read what is selected in");
    expect(describeCall("get_context", false, "allow")).toBe("read which file is open in Writ");
    expect(describeCall("get_context", false, "refuse")).toBe(
      "asked to read which file is open in Writ",
    );
    expect(isReadAction("get_context")).toBe(false);
  });

  it("says what the grant covers in words", () => {
    expect(describeContextTools(["get_context"])).toBe(
      "see the file open in Writ and the text you selected",
    );
  });

  it("counts showing a file at a heading as a read", () => {
    expect(isReadAction("reveal")).toBe(true);
    expect(describeCall("reveal", true, "allow")).toBe("asked to show");
  });
});
