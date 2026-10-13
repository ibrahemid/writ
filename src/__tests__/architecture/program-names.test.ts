import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { PROGRAM_NAMES } from "../../lib/program-names";

const ACTIVITY_RS = readFileSync(resolve(process.cwd(), "crates/writ-core/src/activity.rs"), "utf8");

function serverTable(): Array<[string, string]> {
  const start = ACTIVITY_RS.indexOf("pub const PROGRAM_NAMES");
  expect(start, "PROGRAM_NAMES is declared in crates/writ-core/src/activity.rs").toBeGreaterThan(-1);
  const body = ACTIVITY_RS.slice(start, ACTIVITY_RS.indexOf("];", start));
  return [...body.matchAll(/\("([^"]+)",\s*"([^"]+)"\)/g)].map((match) => [match[1]!, match[2]!]);
}

describe("the app and the server show a known program by one name", () => {
  it("reads the server's table", () => {
    expect(serverTable()).toContainEqual(["claude-code", "Claude Code"]);
  });

  it("holds the same entries in the same order", () => {
    expect([...PROGRAM_NAMES]).toEqual(serverTable());
  });
});
