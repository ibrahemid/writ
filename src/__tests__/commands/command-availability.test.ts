import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// A command that cannot act is kept out of the palette rather than listed and
// then stopped. `note.delete` is the case that pays for it: a tab can hold a
// file opened from somebody else's folder, and Writ never moves that to the
// Trash.

vi.mock("../../stores/global/config", () => ({
  configStore: {
    config: () => ({ commands: { usage: {} } }),
    recordCommandUse: vi.fn(),
  },
}));

import { createCommandProvider } from "../../commands/providers/command-provider";
import { registerCommand, unregisterCommand } from "../../commands/registry";

const REGISTERED = ["note.delete", "note.saveCopy"];

function results(query: string): string[] {
  const provider = createCommandProvider({ listOnEmptyQuery: true });
  const rows = provider.query(query, new AbortController().signal, "commands");
  if (rows instanceof Promise) throw new Error("the command provider answers synchronously");
  return rows.map((result) => result.label);
}

describe("palette command availability", () => {
  let deletable = true;

  beforeEach(() => {
    deletable = true;
    registerCommand({
      id: "note.delete",
      label: "Delete Note",
      scope: "app",
      isAvailable: () => deletable,
      execute: () => {},
    });
    registerCommand({
      id: "note.saveCopy",
      label: "Save a Copy…",
      scope: "app",
      execute: () => {},
    });
  });

  afterEach(() => {
    for (const id of REGISTERED) unregisterCommand(id);
  });

  it("lists a command that can act", () => {
    expect(results("delete")).toContain("Delete Note");
    expect(results("")).toContain("Delete Note");
  });

  it("hides a command that would only be stopped", () => {
    deletable = false;

    expect(results("delete")).not.toContain("Delete Note");
    expect(results("")).not.toContain("Delete Note");
  });

  it("leaves a command that declares nothing alone", () => {
    deletable = false;

    expect(results("copy")).toContain("Save a Copy…");
  });
});
