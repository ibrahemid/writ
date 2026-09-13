import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  TOOL_PHRASES,
  describeReadTools,
  describeWriteTools,
} from "../../components/SettingsModal/SettingsModal";

// The settings row says what a program may do in words the user did not have to
// learn, so every tool the server registers has to be named here. A tool added
// to the server and missing from the table fails this, rather than reaching the
// user as a bare id.

const TOOLS_RS = readFileSync(resolve(process.cwd(), "crates/writ-core/src/tools.rs"), "utf8");

function constantList(name: string): string[] {
  const match = TOOLS_RS.match(new RegExp(`${name}:\\s*&\\[&str\\]\\s*=\\s*&\\[([^\\]]*)\\]`));
  expect(match, `${name} is declared in crates/writ-core/src/tools.rs`).toBeTruthy();
  return [...match![1]!.matchAll(/"([^"]+)"/g)].map((m) => m[1]!);
}

const SERVER_READ = constantList("READ_TOOLS");
const SERVER_WRITE = constantList("WRITE_TOOLS");

describe("the tool table names every tool the server registers", () => {
  it("reads both lists off the server", () => {
    expect(SERVER_READ.length).toBeGreaterThan(0);
    expect(SERVER_WRITE.length).toBeGreaterThan(0);
  });

  it("names every read tool as something to do or to see", () => {
    for (const id of SERVER_READ) {
      const phrase = TOOL_PHRASES[id];
      expect(phrase, `${id} is named in TOOL_PHRASES`).toBeTruthy();
      expect(["do", "see"], id).toContain(phrase!.group);
    }
  });

  it("names every write tool as a clause about writing", () => {
    for (const id of SERVER_WRITE) {
      const phrase = TOOL_PHRASES[id];
      expect(phrase, `${id} is named in TOOL_PHRASES`).toBeTruthy();
      expect(phrase!.group, id).toBe("write");
    }
  });

  it("names nothing the server does not register", () => {
    const registered = new Set([...SERVER_READ, ...SERVER_WRITE]);
    for (const id of Object.keys(TOOL_PHRASES)) {
      expect(registered.has(id), `${id} is registered by the server`).toBe(true);
    }
  });

  it("turns the server's lists into the sentences the row shows", () => {
    expect(describeReadTools(SERVER_READ)).toBe(
      "list, search and open notes, and see their links, properties and tags",
    );
    expect(describeWriteTools(SERVER_WRITE)).toBe(
      "replace a note's text, make a new note, rename a note",
    );
  });

  it("keeps a tool it cannot name, rather than dropping it", () => {
    expect(describeReadTools(["list_notes", "count_notes"])).toBe("list notes, and count_notes");
    expect(describeWriteTools(["create_note", "merge_notes"])).toBe(
      "make a new note, merge_notes",
    );
  });
});
