import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it, expect } from "vitest";

// One feature, one name (NV-28). The palette is searched by the words a person
// has seen elsewhere in the window, so a description naming something no
// surface shows ("workspace", "history rail") is a search that finds nothing.

const APP_TSX = readFileSync(resolve(process.cwd(), "src/App.tsx"), "utf8");
const INBOX_TS = readFileSync(resolve(process.cwd(), "src/stores/global/inbox.ts"), "utf8");

/** Every `description: "…"` string `App.tsx` registers a command with. */
function descriptions(): string[] {
  return Array.from(APP_TSX.matchAll(/description:\s*"([^"]*)"/g), (match) => match[1]);
}

describe("the palette's words for the sidebar", () => {
  it("names no workspace and no rail", () => {
    const offenders = descriptions().filter((text) => /\bworkspace\b|\brail\b/i.test(text));
    expect(offenders).toEqual([]);
  });

  it("describes the sidebar's commands in the sidebar's own words", () => {
    expect(APP_TSX).toContain('description: "Show a folder\'s files in the sidebar"');
    expect(APP_TSX).toContain('description: "Stop showing this folder in the sidebar"');
    expect(APP_TSX).toContain('description: "Show or hide the sidebar"');
  });

  it("calls the list of closed files Recent wherever it is cleared", () => {
    expect(APP_TSX).toContain('label: "Clear Recent"');
    expect(APP_TSX).not.toMatch(/Clear (all )?history/);
  });
});

describe("the watched folder's words", () => {
  it("calls it the watched folder in every message it raises", () => {
    expect(INBOX_TS).not.toMatch(/from the inbox|in inbox/);
    expect(INBOX_TS).toContain("from the watched folder");
    expect(INBOX_TS).toContain("in the watched folder");
  });
});
